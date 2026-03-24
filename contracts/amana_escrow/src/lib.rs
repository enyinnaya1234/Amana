#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, Symbol};

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Event emitted when the contract is successfully initialized.
#[contracttype]
#[derive(Clone, Debug)]
pub struct InitializedEvent {
    /// The administrator address set during initialization.
    pub admin: Address,
    /// Platform fee in basis points (e.g. 100 = 1%).
    pub fee_bps: u32,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------



// ---------------------------------------------------------------------------
// TradeStatus
// ---------------------------------------------------------------------------

/// Represents the various states a trade can be in during its lifecycle.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TradeStatus {
    /// Trade is created but not yet funded by the buyer.
    Created,
    /// Buyer has funded the escrow.
    Funded,
    /// Seller has delivered the goods or services.
    Delivered,
    /// Trade is completed and funds are released to the seller.
    Completed,
    /// A dispute has been raised by either party.
    Disputed,
    /// Trade is cancelled and funds are refunded to the buyer.
    Cancelled,
}

// ---------------------------------------------------------------------------
// Trade
// ---------------------------------------------------------------------------

/// The core data structure representing an escrow trade.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trade {
    /// Unique identifier for the trade.
    pub trade_id: u64,
    /// The buyer's address.
    pub buyer: Address,
    /// The seller's address.
    pub seller: Address,
    /// The trade amount in USDC.
    pub amount_usdc: i128,
    /// The current status of the trade.
    pub status: TradeStatus,
    /// The timestamp when the trade was created.
    pub created_at: u64,
    /// The timestamp when the trade was last updated.
    pub updated_at: u64,
}

// ---------------------------------------------------------------------------
// DataKey — namespaced storage keys
// ---------------------------------------------------------------------------

/// Keys for all storage namespaces used by this contract.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataKey {
    /// Maps a trade ID to a Trade struct in persistent storage.
    Trade(u64),
    /// Boolean flag: whether the contract has been initialized. Stored in instance storage.
    Initialized,
    /// The administrator address set during initialization. Stored in instance storage.
    Admin,
    /// The USDC token contract address. Stored in instance storage.
    UsdcContract,
    /// Platform fee expressed in basis points (e.g. 100 = 1%). Stored in instance storage.
    FeeBps,
}

// ---------------------------------------------------------------------------
// Legacy symbol-based constants (kept for backward-compatible methods)
// ---------------------------------------------------------------------------

const ADMIN: Symbol = symbol_short!("ADMIN");
const TREASURY: Symbol = symbol_short!("TREASURY");
const FEE_BPS: Symbol = symbol_short!("FEE_BPS");
const FUNDS_RELEASED: Symbol = symbol_short!("RELSD");
const DELIVERY_CONFIRMED: Symbol = symbol_short!("DELCNF");
const BPS_DIVISOR: i128 = 10_000;

#[derive(Clone)]
#[contracttype]
pub enum TradeStatus {
    Funded,
    Delivered,
    Completed,
}

#[derive(Clone)]
#[contracttype]
pub struct Trade {
    pub trade_id: u64,
    pub buyer: Address,
    pub seller: Address,
    pub token: Address,
    pub amount: i128,
    pub status: TradeStatus,
    pub delivered_at: Option<u64>,
}

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Trade(u64),
}

#[derive(Clone)]
#[contracttype]
pub struct FundsReleasedEvent {
    pub trade_id: u64,
    pub seller_amount: i128,
    pub fee_amount: i128,
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

    /// Initialize the escrow contract with global platform parameters.
    ///
    /// # Arguments
    /// * `admin`          — The administrator address that owns the contract.
    /// * `usdc_contract`  — The address of the USDC token contract.
    /// * `fee_bps`        — Platform fee in basis points (e.g. 100 = 1%).
    ///
    /// # Panics
    /// Panics with `Error::AlreadyInitialized` if called more than once.
    pub fn initialize(env: Env, admin: Address, usdc_contract: Address, fee_bps: u32) {
        // Idempotency guard: reject any second call.
        if env
            .storage()
            .instance()
            .get::<DataKey, bool>(&DataKey::Initialized)
            .unwrap_or(false)
        {
            panic!("AlreadyInitialized")
        }

        // The caller must authorise itself as the deployer/admin.
        admin.require_auth();

        // Persist the global configuration.
        env.storage()
            .instance()
            .set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::UsdcContract, &usdc_contract);
        env.storage()
            .instance()
            .set(&DataKey::FeeBps, &fee_bps);

        // Mark the contract as initialized so it cannot be called again.
        env.storage()
            .instance()
            .set(&DataKey::Initialized, &true);

        // Emit an Initialized event for indexers / front-ends.
        env.events()
            .publish(("amana", "initialized"), InitializedEvent { admin, fee_bps });
    }

    // -----------------------------------------------------------------------
    // Legacy escrow methods (unchanged)
    // -----------------------------------------------------------------------

    pub fn deposit(env: Env, buyer: Address, seller: Address, amount: i128) {
        buyer.require_auth();
        env.storage().instance().set(&BUYER, &buyer);
        env.storage().instance().set(&SELLER, &seller);
        env.storage().instance().set(&AMOUNT, &amount);
        env.storage().instance().set(&LOCKED, &true);
    }

    pub fn create_trade(
        env: Env,
        trade_id: u64,
        buyer: Address,
        seller: Address,
        token: Address,
        amount: i128,
    ) {
        buyer.require_auth();
        assert!(amount > 0, "amount must be positive");
        let key = DataKey::Trade(trade_id);
        assert!(
            env.storage().persistent().get::<_, Trade>(&key).is_none(),
            "trade already exists"
        );
        env.storage().persistent().set(
            &key,
            &Trade {
                trade_id,
                buyer,
                seller,
                token,
                amount,
                status: TradeStatus::Funded,
                delivered_at: None,
            },
        );
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

        let admin: Address = env.storage().instance().get(&ADMIN).unwrap();
        let invoker = env.invoker();
        if invoker == trade.buyer {
            trade.buyer.require_auth();
        } else if invoker == admin {
            admin.require_auth();
        } else {
            panic!("only buyer or admin can release");
        }

        let fee_bps: u32 = env.storage().instance().get(&FEE_BPS).unwrap();
        let treasury: Address = env.storage().instance().get(&TREASURY).unwrap();
        let fee_amount = trade.amount * fee_bps as i128 / BPS_DIVISOR;
        let seller_amount = trade.amount - fee_amount;

        let token_client = token::Client::new(&env, &trade.token);
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

    pub fn get_trade(env: Env, trade_id: u64) -> Trade {
        let key = DataKey::Trade(trade_id);
        env.storage().persistent().get(&key).unwrap()
    }
}

#[cfg(test)]
mod test {
    extern crate std;

    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Ledger},
        token, Address, Env,
    };

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

    fn setup_trade(env: &Env, amount: i128, fee_bps: u32) -> (Address, Address, Address, Address, u64) {
        let admin = Address::generate(env);
        let buyer = env.invoker();
        let seller = Address::generate(env);
        let treasury = Address::generate(env);
        let trade_id = 1_u64;

        let escrow_id = env.register(EscrowContract, ());
        let client = EscrowContractClient::new(env, &escrow_id);
        let token_id = env.register(MockTokenContract, ());
        let token_client = MockTokenContractClient::new(env, &token_id);

        token_client.mint(&escrow_id, &amount);
        client.initialize(&admin, &treasury, &fee_bps);
        client.create_trade(&trade_id, &buyer, &seller, &token_id, &amount);
        client.confirm_delivery(&trade_id);

        (escrow_id, token_id, seller, treasury, trade_id)
    }

    #[test]
    fn test_release_sends_correct_amount_to_seller() {
        let env = Env::default();
        env.mock_all_auths();
        let amount = 10_000_i128;
        let fee_bps = 100_u32;
        let (escrow_id, token_id, seller, treasury, trade_id) = setup_trade(&env, amount, fee_bps);

        let client = EscrowContractClient::new(&env, &escrow_id);
        let token_client = MockTokenContractClient::new(&env, &token_id);

        client.release_funds(&trade_id);

        assert_eq!(token_client.balance(&seller), 9_900);
        assert_eq!(token_client.balance(&treasury), 100);
        assert_eq!(token_client.balance(&escrow_id), 0);
    }

    #[test]
    fn test_confirm_delivery_transitions_to_delivered() {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().with_mut(|li| li.timestamp = 1_700_000_000);
        let admin = Address::generate(&env);
        let buyer = env.invoker();
        let seller = Address::generate(&env);
        let treasury = Address::generate(&env);
        let amount = 10_000_i128;
        let trade_id = 42_u64;

        let escrow_id = env.register(EscrowContract, ());
        let client = EscrowContractClient::new(&env, &escrow_id);
        let token_id = env.register(MockTokenContract, ());
        let token_client = MockTokenContractClient::new(&env, &token_id);
        token_client.mint(&escrow_id, &amount);

        client.initialize(&admin, &treasury, &100);
        client.create_trade(&trade_id, &buyer, &seller, &token_id, &amount);
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
        let amount = 10_000_i128;
        let trade_id = 43_u64;

        let escrow_id = env.register(EscrowContract, ());
        let client = EscrowContractClient::new(&env, &escrow_id);
        let token_id = env.register(MockTokenContract, ());
        let token_client = MockTokenContractClient::new(&env, &token_id);
        token_client.mint(&escrow_id, &amount);

        client.initialize(&admin, &treasury, &100);
        client.create_trade(&trade_id, &buyer, &seller, &token_id, &amount);
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
        let amount = 10_000_i128;
        let trade_id = 44_u64;

        let escrow_id = env.register(EscrowContract, ());
        let client = EscrowContractClient::new(&env, &escrow_id);
        let token_id = env.register(MockTokenContract, ());
        let token_client = MockTokenContractClient::new(&env, &token_id);
        token_client.mint(&escrow_id, &amount);

        client.initialize(&admin, &treasury, &100);
        client.create_trade(&trade_id, &buyer, &seller, &token_id, &amount);
        client.confirm_delivery(&trade_id);
        client.confirm_delivery(&trade_id);
    }

    #[test]
    fn test_release_sends_correct_fee_to_treasury() {
        let env = Env::default();
        env.mock_all_auths();
        let amount = 50_000_i128;
        let fee_bps = 100_u32;
        let (escrow_id, token_id, _seller, treasury, trade_id) = setup_trade(&env, amount, fee_bps);

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
        let amount = 10_000_i128;
        let trade_id = 9_u64;

        let escrow_id = env.register(EscrowContract, ());
        let client = EscrowContractClient::new(&env, &escrow_id);
        let token_id = env.register(MockTokenContract, ());
        let token_client = MockTokenContractClient::new(&env, &token_id);
        token_client.mint(&escrow_id, &amount);

        client.initialize(&admin, &treasury, &100);
        client.create_trade(&trade_id, &buyer, &seller, &token_id, &amount);
        client.release_funds(&trade_id);
    }

    #[test]
    fn test_fee_calculation_rounds_correctly() {
        let env = Env::default();
        env.mock_all_auths();
        let amount = 101_i128;
        let fee_bps = 100_u32; // 1%
        let (escrow_id, token_id, seller, treasury, trade_id) = setup_trade(&env, amount, fee_bps);

        let client = EscrowContractClient::new(&env, &escrow_id);
        let token_client = MockTokenContractClient::new(&env, &token_id);

        client.release_funds(&trade_id);

        assert_eq!(token_client.balance(&treasury), 1);
        assert_eq!(token_client.balance(&seller), 100);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    // Helper: deploy a fresh contract instance.
    fn setup() -> (Env, soroban_sdk::Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(EscrowContract, ());
        (env, contract_id)
    }

    // -----------------------------------------------------------------------
    // Core data-structure test (from previous issue)
    // -----------------------------------------------------------------------

    #[test]
    fn test_storage_structs() {
        let (env, contract_id) = setup();

        let buyer = Address::generate(&env);
        let seller = Address::generate(&env);

        let trade = Trade {
            trade_id: 1,
            buyer: buyer.clone(),
            seller: seller.clone(),
            amount_usdc: 1000,
            status: TradeStatus::Created,
            created_at: 1234567890,
            updated_at: 1234567890,
        };

        let key = DataKey::Trade(1);

        env.as_contract(&contract_id, || {
            env.storage().persistent().set(&key, &trade);

            let read_trade: Trade = env.storage().persistent().get(&key).unwrap();

            assert_eq!(read_trade.trade_id, 1);
            assert_eq!(read_trade.buyer, buyer);
            assert_eq!(read_trade.seller, seller);
            assert_eq!(read_trade.amount_usdc, 1000);
            assert_eq!(read_trade.status, TradeStatus::Created);
            assert_eq!(read_trade.created_at, 1234567890);
            assert_eq!(read_trade.updated_at, 1234567890);
        });
    }

    // -----------------------------------------------------------------------
    // Initialization tests
    // -----------------------------------------------------------------------

    /// initialize() should succeed on the first call and persist all parameters.
    #[test]
    fn test_initialize_succeeds() {
        let (env, contract_id) = setup();

        let client = EscrowContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let usdc = Address::generate(&env);
        let fee_bps: u32 = 100; // 1 %

        client.initialize(&admin, &usdc, &fee_bps);

        // Verify stored values via as_contract.
        env.as_contract(&contract_id, || {
            let stored_admin: Address = env
                .storage()
                .instance()
                .get(&DataKey::Admin)
                .unwrap();
            let stored_usdc: Address = env
                .storage()
                .instance()
                .get(&DataKey::UsdcContract)
                .unwrap();
            let stored_fee: u32 = env
                .storage()
                .instance()
                .get(&DataKey::FeeBps)
                .unwrap();
            let initialized: bool = env
                .storage()
                .instance()
                .get(&DataKey::Initialized)
                .unwrap();

            assert_eq!(stored_admin, admin);
            assert_eq!(stored_usdc, usdc);
            assert_eq!(stored_fee, 100);
            assert!(initialized);
        });
    }

    /// initialize() must panic when called a second time.
    #[test]
    #[should_panic]
    fn test_initialize_fails_if_called_twice() {
        let (env, contract_id) = setup();

        let client = EscrowContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let usdc = Address::generate(&env);

        // First call — must succeed.
        client.initialize(&admin, &usdc, &100u32);

        // Second call — must panic with AlreadyInitialized.
        client.initialize(&admin, &usdc, &100u32);
    }
}
