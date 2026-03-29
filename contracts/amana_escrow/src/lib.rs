#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, Symbol};

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug)]
pub struct InitializedEvent {
    pub admin: Address,
    pub fee_bps: u32,
}

// ---------------------------------------------------------------------------
// TradeStatus
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TradeStatus {
    Created,
    Funded,
    Delivered,
    Completed,
    Disputed,
    Cancelled,
}

// ---------------------------------------------------------------------------
// Trade
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trade {
    pub trade_id: u64,
    pub buyer: Address,
    pub seller: Address,
    /// Token contract used for this trade.
    pub token: Address,
    /// Trade amount in USDC.
    pub amount: i128,
    pub status: TradeStatus,
    pub delivered_at: Option<u64>,
}

// ---------------------------------------------------------------------------
// DataKey — namespaced storage keys
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataKey {
    Trade(u64),
    Initialized,
    Admin,
    UsdcContract,
    FeeBps,
}

// ---------------------------------------------------------------------------
// Symbol constants
// ---------------------------------------------------------------------------

const FUNDS_RELEASED: Symbol = symbol_short!("RELSD");
const DELIVERY_CONFIRMED: Symbol = symbol_short!("DELCNF");
const TRADE_CREATED: Symbol = symbol_short!("TRDCRT");
const NEXT_TRADE_ID: Symbol = symbol_short!("NXTTRD");
const BPS_DIVISOR: i128 = 10_000;

// ---------------------------------------------------------------------------
// Event structs
// ---------------------------------------------------------------------------

#[derive(Clone)]
#[contracttype]
pub struct FundsReleasedEvent {
    pub trade_id: u64,
    pub seller_amount: i128,
    pub fee_amount: i128,
}

#[derive(Clone)]
#[contracttype]
pub struct TradeCreatedEvent {
    pub trade_id: u64,
    pub buyer: Address,
    pub seller: Address,
    pub amount_usdc: i128,
}

#[derive(Clone)]
#[contracttype]
pub struct DeliveryConfirmedEvent {
    pub trade_id: u64,
    pub delivered_at: u64,
}

// ---------------------------------------------------------------------------
// Contract impl
// ---------------------------------------------------------------------------

#[contract]
pub struct EscrowContract;

#[contractimpl]
impl EscrowContract {
    // -----------------------------------------------------------------------
    // Initialization
    // -----------------------------------------------------------------------

    pub fn initialize(env: Env, admin: Address, usdc_contract: Address, fee_bps: u32) {
        if env
            .storage()
            .instance()
            .get::<DataKey, bool>(&DataKey::Initialized)
            .unwrap_or(false)
        {
            panic!("AlreadyInitialized")
        }

        admin.require_auth();

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::UsdcContract, &usdc_contract);
        env.storage().instance().set(&DataKey::FeeBps, &fee_bps);
        env.storage().instance().set(&DataKey::Initialized, &true);

        env.events()
            .publish(("amana", "initialized"), InitializedEvent { admin, fee_bps });
    }

    // -----------------------------------------------------------------------
    // Trade lifecycle
    // -----------------------------------------------------------------------

    pub fn create_trade(env: Env, buyer: Address, seller: Address, amount_usdc: i128) -> u64 {
        assert!(amount_usdc > 0, "amount_usdc must be greater than zero");
        let invoker = env.invoker();
        assert!(
            invoker == buyer || invoker == seller,
            "only buyer or seller can create trade"
        );
        invoker.require_auth();

        let ledger_seq = env.ledger().sequence() as u64;
        let next_id: u64 = env.storage().instance().get(&NEXT_TRADE_ID).unwrap_or(1_u64);
        let trade_id = (ledger_seq << 32) | next_id;
        env.storage().instance().set(&NEXT_TRADE_ID, &(next_id + 1));

        let key = DataKey::Trade(trade_id);
        assert!(
            env.storage().persistent().get::<_, Trade>(&key).is_none(),
            "trade already exists"
        );

        let token = env.current_contract_address();
        env.storage().persistent().set(
            &key,
            &Trade {
                trade_id,
                buyer: buyer.clone(),
                seller: seller.clone(),
                token,
                amount: amount_usdc,
                status: TradeStatus::Created,
                delivered_at: None,
            },
        );
        env.events().publish(
            (TRADE_CREATED, trade_id),
            TradeCreatedEvent {
                trade_id,
                buyer,
                seller,
                amount_usdc,
            },
        );
        trade_id
    }

    pub fn mark_funded(env: Env, trade_id: u64) {
        let key = DataKey::Trade(trade_id);
        let mut trade: Trade = env.storage().persistent().get(&key).unwrap();
        assert!(
            matches!(trade.status, TradeStatus::Created),
            "trade must be created"
        );
        let invoker = env.invoker();
        assert!(
            invoker == trade.buyer || invoker == trade.seller,
            "only buyer or seller can mark funded"
        );
        invoker.require_auth();
        trade.status = TradeStatus::Funded;
        env.storage().persistent().set(&key, &trade);
    }

    pub fn confirm_delivery(env: Env, trade_id: u64) {
        let key = DataKey::Trade(trade_id);
        let mut trade: Trade = env.storage().persistent().get(&key).unwrap();
        let invoker = env.invoker();
        assert!(invoker == trade.buyer, "only buyer can confirm delivery");
        trade.buyer.require_auth();
        assert!(
            matches!(trade.status, TradeStatus::Funded),
            "trade must be funded"
        );
        let delivered_at = env.ledger().timestamp();
        trade.status = TradeStatus::Delivered;
        trade.delivered_at = Some(delivered_at);
        env.storage().persistent().set(&key, &trade);
        env.events().publish(
            (DELIVERY_CONFIRMED, trade_id),
            DeliveryConfirmedEvent {
                trade_id,
                delivered_at,
            },
        );
    }

    pub fn release_funds(env: Env, trade_id: u64) {
        let key = DataKey::Trade(trade_id);
        let mut trade: Trade = env.storage().persistent().get(&key).unwrap();
        assert!(
            matches!(trade.status, TradeStatus::Delivered),
            "trade must be delivered"
        );

        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        let invoker = env.invoker();
        if invoker == trade.buyer {
            trade.buyer.require_auth();
        } else if invoker == admin {
            admin.require_auth();
        } else {
            panic!("only buyer or admin can release");
        }

        let fee_bps: u32 = env.storage().instance().get(&DataKey::FeeBps).unwrap();
        let treasury: Address = env.storage().instance().get(&DataKey::UsdcContract).unwrap();
        let fee_amount = trade.amount * fee_bps as i128 / BPS_DIVISOR;
        let seller_amount = trade.amount - fee_amount;

        let token_client = soroban_sdk::token::Client::new(&env, &trade.token);
        token_client.transfer(&env.current_contract_address(), &trade.seller, &seller_amount);
        token_client.transfer(&env.current_contract_address(), &treasury, &fee_amount);

        trade.status = TradeStatus::Completed;
        env.storage().persistent().set(&key, &trade);

        env.events().publish(
            (FUNDS_RELEASED, trade_id),
            FundsReleasedEvent {
                trade_id,
                seller_amount,
                fee_amount,
            },
        );
    }

    // -----------------------------------------------------------------------
    // View functions (read-only, no state mutations)
    // -----------------------------------------------------------------------

    pub fn get_trade(env: Env, trade_id: u64) -> Trade {
        let key = DataKey::Trade(trade_id);
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or_else(|| panic!("Trade ID not found"))
    }

    pub fn get_platform_fee(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::FeeBps)
            .unwrap_or_else(|| panic!("Contract not initialized"))
    }

    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic!("Contract not initialized"))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod test {
    extern crate std;

    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Ledger},
        Address, Env,
    };

    // -----------------------------------------------------------------------
    // Mock token contract
    // -----------------------------------------------------------------------

    #[contract]
    struct MockTokenContract;

    #[derive(Clone)]
    #[contracttype]
    enum MockTokenDataKey {
        Balance(Address),
    }

    #[contractimpl]
    impl MockTokenContract {
        pub fn mint(env: Env, to: Address, amount: i128) {
            let key = MockTokenDataKey::Balance(to.clone());
            let current = env.storage().persistent().get::<_, i128>(&key).unwrap_or(0);
            env.storage().persistent().set(&key, &(current + amount));
        }

        pub fn balance(env: Env, owner: Address) -> i128 {
            env.storage()
                .persistent()
                .get(&MockTokenDataKey::Balance(owner))
                .unwrap_or(0)
        }

        pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
            assert!(amount >= 0, "invalid amount");
            let from_key = MockTokenDataKey::Balance(from.clone());
            let to_key = MockTokenDataKey::Balance(to.clone());
            let from_balance = env.storage().persistent().get::<_, i128>(&from_key).unwrap_or(0);
            assert!(from_balance >= amount, "insufficient balance");
            let to_balance = env.storage().persistent().get::<_, i128>(&to_key).unwrap_or(0);
            env.storage().persistent().set(&from_key, &(from_balance - amount));
            env.storage().persistent().set(&to_key, &(to_balance + amount));
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn setup() -> (Env, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(EscrowContract, ());
        (env, contract_id)
    }

    fn setup_initialized() -> (Env, Address, EscrowContractClient<'static>, Address, u32) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(EscrowContract, ());
        let client = EscrowContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let usdc = Address::generate(&env);
        let fee_bps: u32 = 150;
        client.initialize(&admin, &usdc, &fee_bps);
        (env, contract_id, client, admin, fee_bps)
    }

    /// Deploy + initialize + create a trade in Delivered state, ready for release_funds.
    fn setup_trade(env: &Env, amount: i128, fee_bps: u32) -> (Address, Address, Address, Address, u64) {
        let admin = Address::generate(env);
        let buyer = env.invoker();
        let seller = Address::generate(env);
        let treasury = Address::generate(env);

        let escrow_id = env.register(EscrowContract, ());
        let client = EscrowContractClient::new(env, &escrow_id);
        let token_id = env.register(MockTokenContract, ());
        let token_client = MockTokenContractClient::new(env, &token_id);

        token_client.mint(&escrow_id, &amount);
        client.initialize(&admin, &treasury, &fee_bps);
        let trade_id = client.create_trade(&buyer, &seller, &amount);
        {
            let mut trade = client.get_trade(&trade_id);
            trade.token = token_id.clone();
            trade.status = TradeStatus::Funded;
            env.storage().persistent().set(&DataKey::Trade(trade_id), &trade);
        }
        client.confirm_delivery(&trade_id);

        (escrow_id, token_id, seller, treasury, trade_id)
    }

    // -----------------------------------------------------------------------
    // release_funds tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_release_sends_correct_amount_to_seller() {
        let env = Env::default();
        env.mock_all_auths();
        let (escrow_id, token_id, seller, treasury, trade_id) = setup_trade(&env, 10_000, 100);
        let client = EscrowContractClient::new(&env, &escrow_id);
        let token_client = MockTokenContractClient::new(&env, &token_id);
        client.release_funds(&trade_id);
        assert_eq!(token_client.balance(&seller), 9_900);
        assert_eq!(token_client.balance(&treasury), 100);
        assert_eq!(token_client.balance(&escrow_id), 0);
    }

    #[test]
    fn test_release_sends_correct_fee_to_treasury() {
        let env = Env::default();
        env.mock_all_auths();
        let (escrow_id, token_id, _seller, treasury, trade_id) = setup_trade(&env, 50_000, 100);
        let client = EscrowContractClient::new(&env, &escrow_id);
        let token_client = MockTokenContractClient::new(&env, &token_id);
        client.release_funds(&trade_id);
        assert_eq!(token_client.balance(&treasury), 500);
    }

    #[test]
    #[should_panic(expected = "trade must be delivered")]
    fn test_release_fails_if_not_in_delivered_state() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let buyer = Address::generate(&env);
        let seller = Address::generate(&env);
        let treasury = Address::generate(&env);
        let escrow_id = env.register(EscrowContract, ());
        let client = EscrowContractClient::new(&env, &escrow_id);
        let token_id = env.register(MockTokenContract, ());
        let token_client = MockTokenContractClient::new(&env, &token_id);
        token_client.mint(&escrow_id, &10_000);
        client.initialize(&admin, &treasury, &100);
        let trade_id = client.create_trade(&buyer, &seller, &10_000);
        {
            let mut trade = client.get_trade(&trade_id);
            trade.token = token_id.clone();
            env.storage().persistent().set(&DataKey::Trade(trade_id), &trade);
        }
        client.release_funds(&trade_id);
    }

    #[test]
    fn test_fee_calculation_rounds_correctly() {
        let env = Env::default();
        env.mock_all_auths();
        let (escrow_id, token_id, seller, treasury, trade_id) = setup_trade(&env, 101, 100);
        let client = EscrowContractClient::new(&env, &escrow_id);
        let token_client = MockTokenContractClient::new(&env, &token_id);
        client.release_funds(&trade_id);
        assert_eq!(token_client.balance(&treasury), 1);
        assert_eq!(token_client.balance(&seller), 100);
    }

    // -----------------------------------------------------------------------
    // confirm_delivery tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_confirm_delivery_transitions_to_delivered() {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().with_mut(|li| li.timestamp = 1_700_000_000);
        let admin = Address::generate(&env);
        let buyer = env.invoker();
        let seller = Address::generate(&env);
        let treasury = Address::generate(&env);
        let escrow_id = env.register(EscrowContract, ());
        let client = EscrowContractClient::new(&env, &escrow_id);
        let token_id = env.register(MockTokenContract, ());
        let token_client = MockTokenContractClient::new(&env, &token_id);
        token_client.mint(&escrow_id, &10_000);
        client.initialize(&admin, &treasury, &100);
        let trade_id = client.create_trade(&buyer, &seller, &10_000);
        {
            let mut trade = client.get_trade(&trade_id);
            trade.token = token_id.clone();
            trade.status = TradeStatus::Funded;
            env.storage().persistent().set(&DataKey::Trade(trade_id), &trade);
        }
        client.confirm_delivery(&trade_id);
        let trade = client.get_trade(&trade_id);
        assert!(matches!(trade.status, TradeStatus::Delivered));
        assert_eq!(trade.delivered_at, Some(1_700_000_000));
    }

    #[test]
    #[should_panic(expected = "only buyer can confirm delivery")]
    fn test_confirm_delivery_fails_if_not_buyer() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let buyer = Address::generate(&env);
        let seller = Address::generate(&env);
        let treasury = Address::generate(&env);
        let escrow_id = env.register(EscrowContract, ());
        let client = EscrowContractClient::new(&env, &escrow_id);
        let token_id = env.register(MockTokenContract, ());
        let token_client = MockTokenContractClient::new(&env, &token_id);
        token_client.mint(&escrow_id, &10_000);
        client.initialize(&admin, &treasury, &100);
        let trade_id = client.create_trade(&buyer, &seller, &10_000);
        {
            let mut trade = client.get_trade(&trade_id);
            trade.token = token_id.clone();
            trade.status = TradeStatus::Funded;
            env.storage().persistent().set(&DataKey::Trade(trade_id), &trade);
        }
        client.confirm_delivery(&trade_id);
    }

    #[test]
    #[should_panic(expected = "trade must be funded")]
    fn test_confirm_delivery_fails_if_not_in_funded_state() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let buyer = Address::generate(&env);
        let seller = Address::generate(&env);
        let treasury = Address::generate(&env);
        let escrow_id = env.register(EscrowContract, ());
        let client = EscrowContractClient::new(&env, &escrow_id);
        let token_id = env.register(MockTokenContract, ());
        let token_client = MockTokenContractClient::new(&env, &token_id);
        token_client.mint(&escrow_id, &10_000);
        client.initialize(&admin, &treasury, &100);
        let trade_id = client.create_trade(&buyer, &seller, &10_000);
        {
            let mut trade = client.get_trade(&trade_id);
            trade.token = token_id.clone();
            env.storage().persistent().set(&DataKey::Trade(trade_id), &trade);
        }
        client.confirm_delivery(&trade_id);
        client.confirm_delivery(&trade_id);
    }

    // -----------------------------------------------------------------------
    // create_trade tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_trade_returns_id() {
        let env = Env::default();
        env.mock_all_auths();
        let buyer = env.invoker();
        let seller = Address::generate(&env);
        let escrow_id = env.register(EscrowContract, ());
        let client = EscrowContractClient::new(&env, &escrow_id);
        let trade_id = client.create_trade(&buyer, &seller, &10_000);
        assert!(trade_id > 0);
        let trade = client.get_trade(&trade_id);
        assert!(matches!(trade.status, TradeStatus::Created));
    }

    #[test]
    #[should_panic(expected = "amount_usdc must be greater than zero")]
    fn test_create_trade_fails_on_zero_amount() {
        let env = Env::default();
        env.mock_all_auths();
        let buyer = env.invoker();
        let seller = Address::generate(&env);
        let escrow_id = env.register(EscrowContract, ());
        let client = EscrowContractClient::new(&env, &escrow_id);
        client.create_trade(&buyer, &seller, &0);
    }

    #[test]
    fn test_create_trade_emits_event() {
        let env = Env::default();
        env.mock_all_auths();
        let buyer = env.invoker();
        let seller = Address::generate(&env);
        let escrow_id = env.register(EscrowContract, ());
        let client = EscrowContractClient::new(&env, &escrow_id);
        let trade_id = client.create_trade(&buyer, &seller, &5_000);
        let events = env.events().all();
        assert!(events.len() > 0);
        let found = events.iter().any(|event| {
            let topic0: Symbol = event.0.get(0).unwrap();
            let topic1: u64 = event.0.get(1).unwrap();
            topic0 == TRADE_CREATED && topic1 == trade_id
        });
        assert!(found, "expected trade created event");
    }

    // -----------------------------------------------------------------------
    // Initialization tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_initialize_succeeds() {
        let (env, contract_id) = setup();
        let client = EscrowContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let usdc = Address::generate(&env);
        client.initialize(&admin, &usdc, &100u32);
        env.as_contract(&contract_id, || {
            let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
            let stored_usdc: Address = env.storage().instance().get(&DataKey::UsdcContract).unwrap();
            let stored_fee: u32 = env.storage().instance().get(&DataKey::FeeBps).unwrap();
            let initialized: bool = env.storage().instance().get(&DataKey::Initialized).unwrap();
            assert_eq!(stored_admin, admin);
            assert_eq!(stored_usdc, usdc);
            assert_eq!(stored_fee, 100);
            assert!(initialized);
        });
    }

    #[test]
    #[should_panic]
    fn test_initialize_fails_if_called_twice() {
        let (env, contract_id) = setup();
        let client = EscrowContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let usdc = Address::generate(&env);
        client.initialize(&admin, &usdc, &100u32);
        client.initialize(&admin, &usdc, &100u32);
    }

    // -----------------------------------------------------------------------
    // Storage struct test
    // -----------------------------------------------------------------------

    #[test]
    fn test_storage_structs() {
        let (env, contract_id) = setup();
        let buyer = Address::generate(&env);
        let seller = Address::generate(&env);
        let token = Address::generate(&env);
        let trade = Trade {
            trade_id: 1,
            buyer: buyer.clone(),
            seller: seller.clone(),
            token: token.clone(),
            amount: 1000,
            status: TradeStatus::Created,
            delivered_at: None,
        };
        let key = DataKey::Trade(1);
        env.as_contract(&contract_id, || {
            env.storage().persistent().set(&key, &trade);
            let read_trade: Trade = env.storage().persistent().get(&key).unwrap();
            assert_eq!(read_trade.trade_id, 1);
            assert_eq!(read_trade.buyer, buyer);
            assert_eq!(read_trade.seller, seller);
            assert_eq!(read_trade.amount, 1000);
            assert_eq!(read_trade.status, TradeStatus::Created);
            assert_eq!(read_trade.delivered_at, None);
        });
    }

    // -----------------------------------------------------------------------
    // View function tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_trade_returns_correct_data() {
        let (env, contract_id) = setup();
        let buyer = Address::generate(&env);
        let seller = Address::generate(&env);
        let token = Address::generate(&env);
        let trade = Trade {
            trade_id: 42,
            buyer: buyer.clone(),
            seller: seller.clone(),
            token: token.clone(),
            amount: 5_000,
            status: TradeStatus::Created,
            delivered_at: None,
        };
        env.as_contract(&contract_id, || {
            env.storage().persistent().set(&DataKey::Trade(42), &trade);
        });
        let client = EscrowContractClient::new(&env, &contract_id);
        let result = client.get_trade(&42_u64);
        assert_eq!(result.trade_id, 42);
        assert_eq!(result.buyer, buyer);
        assert_eq!(result.seller, seller);
        assert_eq!(result.amount, 5_000);
        assert!(matches!(result.status, TradeStatus::Created));
    }

    #[test]
    #[should_panic(expected = "Trade ID not found")]
    fn test_get_trade_panics_on_invalid_id() {
        let (env, contract_id) = setup();
        let client = EscrowContractClient::new(&env, &contract_id);
        client.get_trade(&9999_u64);
    }

    #[test]
    fn test_get_platform_fee_returns_initialized_value() {
        let (env, _contract_id, client, _admin, fee_bps) = setup_initialized();
        assert_eq!(client.get_platform_fee(), fee_bps);
    }
}
