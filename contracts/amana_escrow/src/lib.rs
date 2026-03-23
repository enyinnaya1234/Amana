#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, Symbol};

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

/// Keys for the contract's different storage spaces.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataKey {
    /// Maps a trade ID to a Trade struct in storage.
    Trade(u64),
}

const SELLER: Symbol = symbol_short!("SELLER");
const BUYER: Symbol = symbol_short!("BUYER");
const AMOUNT: Symbol = symbol_short!("AMOUNT");
const LOCKED: Symbol = symbol_short!("LOCKED");

#[contract]
pub struct EscrowContract;

#[contractimpl]
impl EscrowContract {
    pub fn deposit(env: Env, buyer: Address, seller: Address, amount: i128) {
        buyer.require_auth();
        env.storage().instance().set(&BUYER, &buyer);
        env.storage().instance().set(&SELLER, &seller);
        env.storage().instance().set(&AMOUNT, &amount);
        env.storage().instance().set(&LOCKED, &true);
    }

    pub fn release(env: Env, buyer: Address) {
        buyer.require_auth();
        let stored_buyer: Address = env.storage().instance().get(&BUYER).unwrap();
        assert!(buyer == stored_buyer, "only buyer can release");
        env.storage().instance().set(&LOCKED, &false);
    }

    pub fn refund(env: Env, seller: Address) {
        seller.require_auth();
        let stored_seller: Address = env.storage().instance().get(&SELLER).unwrap();
        assert!(seller == stored_seller, "only seller can refund");
        env.storage().instance().set(&LOCKED, &false);
    }

    pub fn status(env: Env) -> bool {
        env.storage().instance().get(&LOCKED).unwrap_or(false)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    #[test]
    fn test_storage_structs() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(EscrowContract, ());

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
}
