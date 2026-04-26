#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, Env, String, Symbol, Vec,
};

#[cfg(test)]
mod test;

// ════════════════════════════════════════════════════════════════════
//  Storage Types
// ════════════════════════════════════════════════════════════════════

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Contract administrator.
    Admin,
    /// Governance role flag for an address.
    GovernanceRole(Address),
    /// Delegated admin flag for an address (lender management only).
    DelegatedAdmin(Address),
    /// Lender record by address.
    Lender(Address),
    /// List of all lender addresses that have ever been added.
    LenderList,
}

/// Lender status.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum LenderStatus {
    /// Lender is active and can rely on Veritasor attestations.
    Active,
    /// Lender has been removed.
    Removed,
}

/// Human-readable lender metadata.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct LenderMetadata {
    /// Display name.
    pub name: String,
    /// Optional website or documentation URL.
    pub url: String,
    /// Free-form notes.
    pub notes: String,
}

/// Full lender record.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Lender {
    /// Lender address.
    pub address: Address,
    /// Access tier (1+). Tier 0 is treated as no access.
    pub tier: u32,
    /// Current status.
    pub status: LenderStatus,
    /// Metadata.
    pub metadata: LenderMetadata,
    /// Ledger sequence when first added.
    pub added_at: u32,
    /// Ledger sequence when last updated.
    pub updated_at: u32,
    /// Address that last updated the record.
    pub updated_by: Address,
}

// ════════════════════════════════════════════════════════════════════
//  Events
// ════════════════════════════════════════════════════════════════════

const TOPIC_LENDER_SET: Symbol = symbol_short!("lnd_set");
const TOPIC_LENDER_REMOVED: Symbol = symbol_short!("lnd_rem");
const TOPIC_GOV_GRANTED: Symbol = symbol_short!("gov_add");
const TOPIC_GOV_REVOKED: Symbol = symbol_short!("gov_del");
/// Delegated admin events.
const TOPIC_DELEGATED_ADMIN_GRANTED: Symbol = symbol_short!("dlg_ad_gr");
const TOPIC_DELEGATED_ADMIN_REVOKED: Symbol = symbol_short!("dlg_ad_rv");

#[contracttype]
#[derive(Clone, Debug)]
pub struct LenderEvent {
    pub lender: Address,
    pub tier: u32,
    pub status: LenderStatus,
    pub changed_by: Address,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct GovernanceEvent {
    pub account: Address,
    pub enabled: bool,
    pub changed_by: Address,
}

/// Delegated admin event.
#[contracttype]
#[derive(Clone, Debug)]
pub struct DelegatedAdminEvent {
    pub account: Address,
    pub enabled: bool,
    pub changed_by: Address,
}

// ════════════════════════════════════════════════════════════════════
//  Contract
// ════════════════════════════════════════════════════════════════════

#[contract]
pub struct LenderAccessListContract;

#[contractimpl]
impl LenderAccessListContract {
    /// Initialize the contract with an admin address.
    ///
    /// Governance role is automatically granted to `admin`.
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        admin.require_auth();

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::GovernanceRole(admin.clone()), &true);
        env.storage()
            .instance()
            .set(&DataKey::LenderList, &Vec::<Address>::new(&env));
    }

    /// Grant governance role to an address. Only admin.
    pub fn grant_governance(env: Env, admin: Address, account: Address) {
        Self::require_admin(&env, &admin);
        env.storage()
            .instance()
            .set(&DataKey::GovernanceRole(account.clone()), &true);

        env.events().publish(
            (TOPIC_GOV_GRANTED,),
            GovernanceEvent {
                account,
                enabled: true,
                changed_by: admin,
            },
        );
    }

    /// Revoke governance role from an address. Only admin.
    pub fn revoke_governance(env: Env, admin: Address, account: Address) {
        Self::require_admin(&env, &admin);
        env.storage()
            .instance()
            .set(&DataKey::GovernanceRole(account.clone()), &false);

        env.events().publish(
            (TOPIC_GOV_REVOKED,),
            GovernanceEvent {
                account,
                enabled: false,
                changed_by: admin,
            },
        );
    }

    /// Grant delegated admin role for lender management. Only admin.
    pub fn grant_delegated_admin(env: Env, admin: Address, account: Address) {
        Self::require_admin(&env, &admin);
        env.storage()
            .instance()
            .set(&DataKey::DelegatedAdmin(account.clone()), &true);

        env.events().publish(
            (TOPIC_DELEGATED_ADMIN_GRANTED,),
            DelegatedAdminEvent {
                account,
                enabled: true,
                changed_by: admin,
            },
        );
    }

    /// Revoke delegated admin role. Only admin.
    pub fn revoke_delegated_admin(env: Env, admin: Address, account: Address) {
        Self::require_admin(&env, &admin);
        env.storage()
            .instance()
            .set(&DataKey::DelegatedAdmin(account.clone()), &false);

        env.events().publish(
            (TOPIC_DELEGATED_ADMIN_REVOKED,),
            DelegatedAdminEvent {
                account,
                enabled: false,
                changed_by: admin,
            },
        );
    }

    /// Add or update a lender.
    ///
    /// Access tiers:
    /// - tier 0: no access (treated as removed/disabled)
    /// - tier 1+: can rely on Veritasor attestations for lender-facing operations
    ///
    /// Lender admin (governance or delegated admin) can call.
    pub fn set_lender(env: Env, caller: Address, lender: Address, tier: u32, metadata: LenderMetadata) {
        Self::require_lender_admin(&env, &caller);

        let now = env.ledger().sequence();
        let key = DataKey::Lender(lender.clone());

        let (added_at, status) = if let Some(existing) = env.storage().instance().get::<_, Lender>(&key) {
            (
                existing.added_at,
                if tier == 0 { LenderStatus::Removed } else { LenderStatus::Active },
            )
        } else {
            Self::append_lender_to_list(&env, &lender);
            (now, if tier == 0 { LenderStatus::Removed } else { LenderStatus::Active })
        };

        let record = Lender {
            address: lender.clone(),
            tier,
            status: status.clone(),
            metadata,
            added_at,
            updated_at: now,
            updated_by: caller.clone(),
        };

        env.storage().instance().set(&key, &record);

        env.events().publish(
            (TOPIC_LENDER_SET,),
            LenderEvent {
                lender,
                tier,
                status,
                changed_by: caller,
            },
        );
    }

    /// Remove a lender from the allowlist (sets status to Removed and tier to 0).
    /// Lender admin (governance or delegated admin) can call.
    pub fn remove_lender(env: Env, caller: Address, lender: Address) {
        Self::require_lender_admin(&env, &caller);

        let key = DataKey::Lender(lender.clone());
        let mut record: Lender = env
            .storage()
            .instance()
            .get(&key)
            .expect("lender not found");

        record.tier = 0;
        record.status = LenderStatus::Removed;
        record.updated_at = env.ledger().sequence();
        record.updated_by = caller.clone();

        env.storage().instance().set(&key, &record);

        env.events().publish(
            (TOPIC_LENDER_REMOVED,),
            LenderEvent {
                lender,
                tier: 0,
                status: LenderStatus::Removed,
                changed_by: caller,
            },
        );
    }

    /// Get lender record.
    pub fn get_lender(env: Env, lender: Address) -> Option<Lender> {
        env.storage().instance().get(&DataKey::Lender(lender))
    }

    /// Check if a lender is active and has tier >= `min_tier`.
    pub fn is_allowed(env: Env, lender: Address, min_tier: u32) -> bool {
        if min_tier == 0 {
            return true;
        }

        if let Some(record) = Self::get_lender(env, lender) {
            record.status == LenderStatus::Active && record.tier >= min_tier
        } else {
            false
        }
    }

    /// Get all lenders that have ever been added (including removed).
    pub fn get_all_lenders(env: Env) -> Vec<Address> {
        env.storage()
            .instance()
            .get(&DataKey::LenderList)
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Get all active lenders.
    pub fn get_active_lenders(env: Env) -> Vec<Address> {
        let all = Self::get_all_lenders(env.clone());
        let mut out = Vec::new(&env);

        for i in 0..all.len() {
            let addr = all.get(i).unwrap();
            if let Some(record) = Self::get_lender(env.clone(), addr.clone()) {
                if record.status == LenderStatus::Active && record.tier > 0 {
                    out.push_back(addr);
                }
            }
        }

        out
    }

    /// Get contract admin.
    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("not initialized")
    }

    /// Check governance role.
    pub fn has_governance(env: Env, account: Address) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::GovernanceRole(account))
            .unwrap_or(false)
    }

    /// Check delegated admin role.
    pub fn has_delegated_admin(env: Env, account: Address) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::DelegatedAdmin(account))
            .unwrap_or(false)
    }

    fn append_lender_to_list(env: &Env, lender: &Address) {
        let mut list: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::LenderList)
            .unwrap_or_else(|| Vec::new(env));

        let mut found = false;
        for i in 0..list.len() {
            if list.get(i).unwrap() == *lender {
                found = true;
                break;
            }
        }

        if !found {
            list.push_back(lender.clone());
            env.storage().instance().set(&DataKey::LenderList, &list);
        }
    }

    fn require_admin(env: &Env, caller: &Address) {
        caller.require_auth();
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("not initialized");
        assert!(*caller == admin, "caller is not admin");
    }

    /// Require caller has governance OR delegated admin role for lender management.
    fn require_lender_admin(env: &Env, caller: &Address) {
        caller.require_auth();
        let has_gov: bool = env
            .storage()
            .instance()
            .get(&DataKey::GovernanceRole(caller.clone()))
            .unwrap_or(false);
        let has_del: bool = env
            .storage()
            .instance()
            .get(&DataKey::DelegatedAdmin(caller.clone()))
            .unwrap_or(false);
        assert!(has_gov || has_del, "caller lacks lender admin privileges");
    }

    fn require_governance(env: &Env, caller: &Address) {
        caller.require_auth();
        let ok: bool = env
            .storage()
            .instance()
            .get(&DataKey::GovernanceRole(caller.clone()))
            .unwrap_or(false);
        assert!(ok, "caller does not have governance role");
    }
}
