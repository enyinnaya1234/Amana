#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, token, Address, Env, Symbol};

const ADMIN: Symbol = symbol_short!("ADMIN");
const TREASURY: Symbol = symbol_short!("TREASURY");
const FEE_BPS: Symbol = symbol_short!("FEE_BPS");
const BUYER: Symbol = symbol_short!("BUYER");
const SELLER: Symbol = symbol_short!("SELLER");
const AMOUNT: Symbol = symbol_short!("AMOUNT");
const LOCKED: Symbol = symbol_short!("LOCKED");
const FUNDS_RELEASED: Symbol = symbol_short!("RELSD");
const DELIVERY_CONFIRMED: Symbol = symbol_short!("DELCNF");
const TRADE_CREATED: Symbol = symbol_short!("TRDCRT");
const NEXT_TRADE_ID: Symbol = symbol_short!("NXTTRD");
const BPS_DIVISOR: i128 = 10_000;

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TradeStatus {
    Created,
    Funded,
    Delivered,
    Completed,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trade {
    pub trade_id: u64,
    pub buyer: Address,
    pub seller: Address,
    pub token: Address,
    pub amount: i128,
    pub status: TradeStatus,
    pub delivered_at: Option<u64>,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataKey {
    Trade(u64),
    Initialized,
    Admin,
    UsdcContract,
    FeeBps,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct InitializedEvent {
    pub admin: Address,
    pub fee_bps: u32,
}

#[contracttype]
#[derive(Clone)]
pub struct FundsReleasedEvent {
    pub trade_id: u64,
    pub seller_amount: i128,
    pub fee_amount: i128,
}

#[contracttype]
#[derive(Clone)]
pub struct TradeCreatedEvent {
    pub trade_id: u64,
    pub buyer: Address,
    pub seller: Address,
    pub amount_usdc: i128,
}

#[contracttype]
#[derive(Clone)]
pub struct DeliveryConfirmedEvent {
    pub trade_id: u64,
    pub delivered_at: u64,
}

#[contract]
pub struct EscrowContract;

#[contractimpl]
impl EscrowContract {
    pub fn initialize(env: Env, admin: Address, treasury: Address, fee_bps: u32) {
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
        env.storage()
            .instance()
            .set(&DataKey::UsdcContract, &treasury);
        env.storage().instance().set(&DataKey::FeeBps, &fee_bps);
        env.storage().instance().set(&DataKey::Initialized, &true);

        env.storage().instance().set(&ADMIN, &admin);
        env.storage().instance().set(&TREASURY, &treasury);
        env.storage().instance().set(&FEE_BPS, &fee_bps);

        env.events()
            .publish(("amana", "initialized"), InitializedEvent { admin, fee_bps });
    }

    pub fn deposit(env: Env, buyer: Address, seller: Address, amount: i128) {
        buyer.require_auth();
        env.storage().instance().set(&BUYER, &buyer);
        env.storage().instance().set(&SELLER, &seller);
        env.storage().instance().set(&AMOUNT, &amount);
        env.storage().instance().set(&LOCKED, &true);
    }

    pub fn create_trade(env: Env, buyer: Address, seller: Address, amount_usdc: i128) -> u64 {
        assert!(amount_usdc > 0, "amount_usdc must be greater than zero");
        buyer.require_auth();

        let ledger_seq = env.ledger().sequence() as u64;
        let next_id: u64 = env.storage().instance().get(&NEXT_TRADE_ID).unwrap_or(1_u64);
        let trade_id = (ledger_seq << 32) | next_id;
        env.storage().instance().set(&NEXT_TRADE_ID, &(next_id + 1));

        let key = DataKey::Trade(trade_id);
        assert!(
            env.storage().persistent().get::<_, Trade>(&key).is_none(),
            "trade already exists"
        );

        let token: Address = env
            .storage()
            .instance()
            .get(&DataKey::UsdcContract)
            .unwrap_or(env.current_contract_address());
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

        trade.buyer.require_auth();
        trade.status = TradeStatus::Funded;
        env.storage().persistent().set(&key, &trade);
    }

    pub fn confirm_delivery(env: Env, trade_id: u64) {
        let key = DataKey::Trade(trade_id);
        let mut trade: Trade = env.storage().persistent().get(&key).unwrap();

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
        admin.require_auth();

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
        testutils::{Address as _, Events, Ledger, MockAuth, MockAuthInvoke},
        contract, contractimpl, contracttype, Address, Env, IntoVal,
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

    fn setup_base_env() -> (Env, Address, Address, Address, Address, Address) {
        let env = Env::default();
        let escrow_id = env.register(EscrowContract, ());
        let admin = Address::generate(&env);
        let buyer = Address::generate(&env);
        let seller = Address::generate(&env);
        let treasury = Address::generate(&env);
        (env, escrow_id, admin, buyer, seller, treasury)
    }

    fn create_env_with_trade() -> (Env, Address, Address, Address, Address, Address, Address, i128, u64) {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().with_mut(|li| {
            li.sequence_number = 7;
            li.timestamp = 1_700_000_000;
        });

        let escrow_id = env.register(EscrowContract, ());
        let token_id = env.register(MockTokenContract, ());
        let admin = Address::generate(&env);
        let buyer = Address::generate(&env);
        let seller = Address::generate(&env);
        let treasury = Address::generate(&env);
        let amount = 10_000_i128;

        let client = EscrowContractClient::new(&env, &escrow_id);
        let token_client = MockTokenContractClient::new(&env, &token_id);
        token_client.mint(&escrow_id, &amount);

        client.initialize(&admin, &token_id, &100);
        let trade_id = client.create_trade(&buyer, &seller, &amount);

        (env, escrow_id, token_id, admin, buyer, seller, treasury, amount, trade_id)
    }

    #[test]
    fn initialize_happy_path() {
        let (env, escrow_id, admin, _buyer, _seller, treasury) = setup_base_env();
        let fee_bps = 100_u32;
        let client = EscrowContractClient::new(&env, &escrow_id);

        client
            .mock_auths(&[MockAuth {
                address: &admin,
                invoke: &MockAuthInvoke {
                    contract: &escrow_id,
                    fn_name: "initialize",
                    args: (&admin, &treasury, &fee_bps).into_val(&env),
                    sub_invokes: &[],
                },
            }])
            .initialize(&admin, &treasury, &fee_bps);
    }

    #[test]
    #[should_panic(expected = "AlreadyInitialized")]
    fn initialize_fails_if_called_twice() {
        let (env, escrow_id, admin, _buyer, _seller, treasury) = setup_base_env();
        let fee_bps = 100_u32;
        let client = EscrowContractClient::new(&env, &escrow_id);

        client.mock_all_auths().initialize(&admin, &treasury, &fee_bps);
        client.mock_all_auths().initialize(&admin, &treasury, &fee_bps);
    }

    #[test]
    fn deposit_happy_path() {
        let (env, escrow_id, _admin, buyer, seller, _treasury) = setup_base_env();
        let amount = 2_500_i128;
        let client = EscrowContractClient::new(&env, &escrow_id);

        client
            .mock_auths(&[MockAuth {
                address: &buyer,
                invoke: &MockAuthInvoke {
                    contract: &escrow_id,
                    fn_name: "deposit",
                    args: (&buyer, &seller, &amount).into_val(&env),
                    sub_invokes: &[],
                },
            }])
            .deposit(&buyer, &seller, &amount);
    }

    #[test]
    fn create_trade_happy_path() {
        let (env, escrow_id, admin, buyer, seller, treasury) = setup_base_env();
        let amount = 10_000_i128;
        let client = EscrowContractClient::new(&env, &escrow_id);
        env.mock_all_auths();

        client.initialize(&admin, &treasury, &100);
        let trade_id = client.create_trade(&buyer, &seller, &amount);

        assert!(trade_id > 0);
    }

    #[test]
    #[should_panic(expected = "amount_usdc must be greater than zero")]
    fn create_trade_fails_zero_amount() {
        let (env, escrow_id, _admin, buyer, seller, _treasury) = setup_base_env();
        let client = EscrowContractClient::new(&env, &escrow_id);

        client.mock_all_auths().create_trade(&buyer, &seller, &0);
    }

    #[test]
    #[should_panic(expected = "Unauthorized function call for address")]
    fn create_trade_fails_wrong_caller() {
        let (env, escrow_id, _admin, buyer, seller, _treasury) = setup_base_env();
        let amount = 10_000_i128;
        let client = EscrowContractClient::new(&env, &escrow_id);

        client
            .mock_auths(&[MockAuth {
                address: &seller,
                invoke: &MockAuthInvoke {
                    contract: &escrow_id,
                    fn_name: "create_trade",
                    args: (&buyer, &seller, &amount).into_val(&env),
                    sub_invokes: &[],
                },
            }])
            .create_trade(&buyer, &seller, &amount);
    }

    #[test]
    fn mark_funded_happy_path() {
        let (env, escrow_id, admin, buyer, seller, treasury) = setup_base_env();
        let amount = 10_000_i128;
        let client = EscrowContractClient::new(&env, &escrow_id);
        env.mock_all_auths();

        client.initialize(&admin, &treasury, &100);
        let trade_id = client.create_trade(&buyer, &seller, &amount);
        client.mark_funded(&trade_id);
    }

    #[test]
    #[should_panic(expected = "trade must be created")]
    fn mark_funded_fails_wrong_status() {
        let (env, escrow_id, admin, buyer, seller, treasury) = setup_base_env();
        let amount = 10_000_i128;
        let client = EscrowContractClient::new(&env, &escrow_id);
        env.mock_all_auths();

        client.initialize(&admin, &treasury, &100);
        let trade_id = client.create_trade(&buyer, &seller, &amount);
        client.mark_funded(&trade_id);
        client.mark_funded(&trade_id);
    }

    #[test]
    #[should_panic(expected = "Unauthorized function call for address")]
    fn mark_funded_fails_wrong_caller() {
        let (env, escrow_id, admin, buyer, seller, treasury) = setup_base_env();
        let amount = 10_000_i128;
        let client = EscrowContractClient::new(&env, &escrow_id);
        client.mock_all_auths().initialize(&admin, &treasury, &100);
        let trade_id = client.mock_all_auths().create_trade(&buyer, &seller, &amount);

        client
            .mock_auths(&[MockAuth {
                address: &seller,
                invoke: &MockAuthInvoke {
                    contract: &escrow_id,
                    fn_name: "mark_funded",
                    args: (&trade_id,).into_val(&env),
                    sub_invokes: &[],
                },
            }])
            .mark_funded(&trade_id);
    }

    #[test]
    fn confirm_delivery_happy_path() {
        let (env, escrow_id, admin, buyer, seller, treasury) = setup_base_env();
        let amount = 10_000_i128;
        let client = EscrowContractClient::new(&env, &escrow_id);
        env.mock_all_auths();

        client.initialize(&admin, &treasury, &100);
        let trade_id = client.create_trade(&buyer, &seller, &amount);
        client.mark_funded(&trade_id);
        client.confirm_delivery(&trade_id);
    }

    #[test]
    #[should_panic(expected = "trade must be funded")]
    fn confirm_delivery_fails_wrong_status() {
        let (env, escrow_id, admin, buyer, seller, treasury) = setup_base_env();
        let amount = 10_000_i128;
        let client = EscrowContractClient::new(&env, &escrow_id);
        env.mock_all_auths();

        client.initialize(&admin, &treasury, &100);
        let trade_id = client.create_trade(&buyer, &seller, &amount);
        client.confirm_delivery(&trade_id);
    }

    #[test]
    #[should_panic(expected = "Unauthorized function call for address")]
    fn confirm_delivery_fails_wrong_caller() {
        let (env, escrow_id, admin, buyer, seller, treasury) = setup_base_env();
        let amount = 10_000_i128;
        let client = EscrowContractClient::new(&env, &escrow_id);
        client.mock_all_auths().initialize(&admin, &treasury, &100);
        let trade_id = client.mock_all_auths().create_trade(&buyer, &seller, &amount);
        client.mock_all_auths().mark_funded(&trade_id);

        client
            .mock_auths(&[MockAuth {
                address: &seller,
                invoke: &MockAuthInvoke {
                    contract: &escrow_id,
                    fn_name: "confirm_delivery",
                    args: (&trade_id,).into_val(&env),
                    sub_invokes: &[],
                },
            }])
            .confirm_delivery(&trade_id);
    }

    #[test]
    fn release_funds_happy_path_multi_step_flow() {
        let (env, escrow_id, token_id, admin, buyer, seller, _treasury, amount, trade_id) =
            create_env_with_trade();
        let client = EscrowContractClient::new(&env, &escrow_id);
        let token_client = MockTokenContractClient::new(&env, &token_id);

        client.deposit(&buyer, &seller, &amount);
        client.mark_funded(&trade_id);
        client.confirm_delivery(&trade_id);
        client.release_funds(&trade_id);

        let seller_balance = token_client.balance(&seller);
        let fee_holder_balance = token_client.balance(&token_id);
        assert_eq!(seller_balance, 9_900);
        assert_eq!(fee_holder_balance, 100);

        let _ = (env, admin);
    }

    #[test]
    #[should_panic(expected = "trade must be delivered")]
    fn release_funds_fails_wrong_status() {
        let (env, escrow_id, admin, buyer, seller, treasury) = setup_base_env();
        let amount = 10_000_i128;
        let client = EscrowContractClient::new(&env, &escrow_id);
        env.mock_all_auths();

        client.initialize(&admin, &treasury, &100);
        let trade_id = client.create_trade(&buyer, &seller, &amount);
        client.release_funds(&trade_id);
    }

    #[test]
    #[should_panic(expected = "Unauthorized function call for address")]
    fn release_funds_fails_wrong_caller() {
        let (env, escrow_id, admin, buyer, seller, _treasury) = setup_base_env();
        let amount = 10_000_i128;
        let token_id = env.register(MockTokenContract, ());
        let client = EscrowContractClient::new(&env, &escrow_id);
        let token_client = MockTokenContractClient::new(&env, &token_id);

        token_client.mint(&escrow_id, &amount);
        client.mock_all_auths().initialize(&admin, &token_id, &100);
        let trade_id = client.mock_all_auths().create_trade(&buyer, &seller, &amount);
        client.mock_all_auths().mark_funded(&trade_id);
        client.mock_all_auths().confirm_delivery(&trade_id);

        client
            .mock_auths(&[MockAuth {
                address: &buyer,
                invoke: &MockAuthInvoke {
                    contract: &escrow_id,
                    fn_name: "release_funds",
                    args: (&trade_id,).into_val(&env),
                    sub_invokes: &[],
                },
            }])
            .release_funds(&trade_id);
    }

    #[test]
    fn release_funds_rounding_path() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let buyer = Address::generate(&env);
        let seller = Address::generate(&env);

        let escrow_id = env.register(EscrowContract, ());
        let token_id = env.register(MockTokenContract, ());
        let client = EscrowContractClient::new(&env, &escrow_id);
        let token_client = MockTokenContractClient::new(&env, &token_id);

        token_client.mint(&escrow_id, &102);
        client.initialize(&admin, &token_id, &100);
        let trade_id = client.create_trade(&buyer, &seller, &101);
        client.mark_funded(&trade_id);
        client.confirm_delivery(&trade_id);
        client.release_funds(&trade_id);

        assert_eq!(token_client.balance(&seller), 100);
        assert_eq!(token_client.balance(&token_id), 1);
    }

    #[test]
    fn get_trade_happy_path() {
        let (env, escrow_id, admin, buyer, seller, treasury) = setup_base_env();
        let amount = 10_000_i128;
        let client = EscrowContractClient::new(&env, &escrow_id);
        env.mock_all_auths();

        client.initialize(&admin, &treasury, &100);
        let trade_id = client.create_trade(&buyer, &seller, &amount);

        let trade = client.get_trade(&trade_id);
        assert_eq!(trade.trade_id, trade_id);
    }

    #[test]
    fn create_trade_emits_event() {
        let (env, escrow_id, admin, buyer, seller, treasury) = setup_base_env();
        let amount = 5_000_i128;
        let client = EscrowContractClient::new(&env, &escrow_id);

        client.mock_all_auths().initialize(&admin, &treasury, &100);
        client.mock_all_auths().create_trade(&buyer, &seller, &amount);

        let events = env.events().all();
        assert!(events.events().len() > 0);
    }

    #[test]
    fn initialize_emits_event() {
        let (env, escrow_id, admin, _buyer, _seller, treasury) = setup_base_env();
        let fee_bps = 100_u32;
        let client = EscrowContractClient::new(&env, &escrow_id);

        client.mock_all_auths().initialize(&admin, &treasury, &fee_bps);

        let events = env.events().all();
        assert!(events.events().len() > 0);
    }

    #[test]
    #[should_panic(expected = "called `Option::unwrap()` on a `None` value")]
    fn get_trade_fails_for_unknown_trade() {
        let (env, escrow_id, _admin, _buyer, _seller, _treasury) = setup_base_env();
        let client = EscrowContractClient::new(&env, &escrow_id);

        client.get_trade(&999_u64);
    }

    #[test]
    fn trade_ids_increment() {
        let (env, escrow_id, admin, buyer, seller, treasury) = setup_base_env();
        let client = EscrowContractClient::new(&env, &escrow_id);
        client.mock_all_auths().initialize(&admin, &treasury, &100);

        let first = client.mock_all_auths().create_trade(&buyer, &seller, &100);
        let second = client.mock_all_auths().create_trade(&buyer, &seller, &200);

        assert!(second > first);
    }

    #[test]
    fn release_fee_changes_with_bps() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let buyer = Address::generate(&env);
        let seller = Address::generate(&env);

        let escrow_id = env.register(EscrowContract, ());
        let token_id = env.register(MockTokenContract, ());
        let client = EscrowContractClient::new(&env, &escrow_id);
        let token_client = MockTokenContractClient::new(&env, &token_id);

        token_client.mint(&escrow_id, &10_000);
        client.initialize(&admin, &token_id, &250);
        let trade_id = client.create_trade(&buyer, &seller, &10_000);
        client.mark_funded(&trade_id);
        client.confirm_delivery(&trade_id);
        client.release_funds(&trade_id);

        assert_eq!(token_client.balance(&seller), 9_750);
        assert_eq!(token_client.balance(&token_id), 250);
    }
}
