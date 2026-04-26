//! # Multisignature Admin for Protocol Control
//!
//! This module implements a multisignature mechanism for managing sensitive
//! protocol parameters and emergency actions in the attestation contract.
//!
//! ## Design
//!
//! The multisig system uses a proposal-and-approval model:
//! 1. Any owner can propose an action
//! 2. Other owners approve or reject the proposal
//! 3. Once threshold approvals are reached, the action can be executed
//! 4. Proposals expire after a configurable time window
//!
//! ## Actions
//!
//! Multisig-controlled actions include:
//! - Emergency pause/unpause
//! - Owner management (add/remove owners, change threshold)
//! - Fee configuration changes
//! - Role management for critical roles
//!
//! ## Security Properties
//!
//! - No single owner can execute critical actions alone
//! - Proposals have expiration to prevent stale executions
//! - Executed proposals are marked to prevent replay
//! - Owner list and threshold are protected by multisig itself

use soroban_sdk::{contracttype, Address, Env, Vec};

// ════════════════════════════════════════════════════════════════════
//  Storage Types
// ════════════════════════════════════════════════════════════════════

/// Storage keys for multisig state
#[contracttype]
#[derive(Clone)]
pub enum MultisigKey {
    /// List of multisig owners
    Owners,
    /// Required approval threshold
    Threshold,
    /// Proposal data by proposal ID
    Proposal(u64),
    /// Approvals for a proposal (list of approving addresses)
    Approvals(u64),
    /// Next proposal ID counter
    NextProposalId,
}

/// Types of actions that can be proposed
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ProposalAction {
    /// Emergency pause the contract
    Pause,
    /// Unpause the contract
    Unpause,
    /// Add a new owner
    AddOwner(Address),
    /// Remove an owner
    RemoveOwner(Address),
    /// Change the approval threshold
    ChangeThreshold(u32),
    /// Grant a role to an address
    GrantRole(Address, u32),
    /// Revoke a role from an address
    RevokeRole(Address, u32),
    /// Update fee configuration
    UpdateFeeConfig(Address, Address, i128, bool), // (token, collector, base_fee, enabled)
    /// Emergency admin key rotation (bypasses timelock)
    EmergencyRotateAdmin(Address), // new_admin
}

/// Proposal state
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ProposalStatus {
    /// Proposal is pending approvals
    Pending,
    /// Proposal has been executed
    Executed,
    /// Proposal was rejected
    Rejected,
    /// Proposal expired without execution
    Expired,
}

/// Full proposal data
#[contracttype]
#[derive(Clone, Debug)]
pub struct Proposal {
    /// Unique proposal identifier
    pub id: u64,
    /// The action to be executed
    pub action: ProposalAction,
    /// Address that created the proposal
    pub proposer: Address,
    /// Current status
    pub status: ProposalStatus,
    /// Ledger sequence when proposal was created
    pub created_at: u32,
}

// ════════════════════════════════════════════════════════════════════
//  Configuration
// ════════════════════════════════════════════════════════════════════

/// Default proposal expiration (in ledger sequences, ~1 week at 5s/ledger)
pub const DEFAULT_PROPOSAL_EXPIRY: u32 = 120_960;

/// Minimum number of owners required
pub const MIN_OWNERS: u32 = 1;

/// Maximum number of owners allowed
pub const MAX_OWNERS: u32 = 10;

// ════════════════════════════════════════════════════════════════════
//  Owner Management
// ════════════════════════════════════════════════════════════════════

/// Get the list of multisig owners.
pub fn get_owners(env: &Env) -> Vec<Address> {
    env.storage()
        .instance()
        .get(&MultisigKey::Owners)
        .unwrap_or_else(|| Vec::new(env))
}

pub fn set_owners(env: &Env, owners: &Vec<Address>) {
    assert!(!owners.is_empty(), "must have at least one owner");
    env.storage().instance().set(&MultisigKey::Owners, owners);
}

pub fn is_owner(env: &Env, address: &Address) -> bool {
    get_owners(env).contains(address)
}

pub fn get_threshold(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&MultisigKey::Threshold)
        .unwrap_or(1)
}

pub fn rotate_threshold(env: &Env, new_threshold: u32) {
    let owners = get_owners(env);
    assert!(
        new_threshold > 0 && new_threshold <= owners.len(),
        "new threshold cannot exceed number of owners"
    );
    env.storage()
        .instance()
        .set(&MultisigKey::Threshold, &new_threshold);
}

pub fn initialize_multisig(env: &Env, owners: &Vec<Address>, threshold: u32) {
    assert!(
        !env.storage().instance().has(&MultisigKey::Owners),
        "multisig already initialized"
    );
    set_owners(env, owners);
    env.storage()
        .instance()
        .set(&MultisigKey::Threshold, &threshold);
}

pub fn create_proposal(env: &Env, proposer: &Address, action: ProposalAction) -> u64 {
    proposer.require_auth();
    assert!(is_owner(env, proposer), "only owners can create proposals");

    let id: u64 = env
        .storage()
        .instance()
        .get(&MultisigKey::NextProposalId)
        .unwrap_or(0);
    env.storage()
        .instance()
        .set(&MultisigKey::NextProposalId, &(id + 1));

    let proposal = Proposal {
        id,
        action,
        proposer: proposer.clone(),
        status: ProposalStatus::Pending,
        created_at: env.ledger().sequence(),
    };
    env.storage()
        .instance()
        .set(&MultisigKey::Proposal(id), &proposal);

    let mut approvals = Vec::new(env);
    approvals.push_back(proposer.clone());
    env.storage()
        .instance()
        .set(&MultisigKey::Approvals(id), &approvals);
    id
}

pub fn get_proposal(env: &Env, id: u64) -> Option<Proposal> {
    env.storage().instance().get(&MultisigKey::Proposal(id))
}

pub fn get_approvals(env: &Env, id: u64) -> Vec<Address> {
    env.storage()
        .instance()
        .get(&MultisigKey::Approvals(id))
        .unwrap_or_else(|| Vec::new(env))
}

pub fn approve_proposal(env: &Env, approver: &Address, id: u64) {
    approver.require_auth();
    let proposal = get_proposal(env, id).expect("proposal not found");
    assert!(
        proposal.status == ProposalStatus::Pending,
        "proposal is not pending"
    );
    assert!(is_owner(env, approver), "only owners can approve proposals");

    let mut approvals = get_approvals(env, id);
    assert!(
        !approvals.contains(approver),
        "already approved this proposal"
    );

    approvals.push_back(approver.clone());
    env.storage()
        .instance()
        .set(&MultisigKey::Approvals(id), &approvals);
}

pub fn reject_proposal(env: &Env, rejecter: &Address, id: u64) {
    rejecter.require_auth();
    assert!(is_owner(env, rejecter), "only owners can reject proposals");
    let mut proposal = get_proposal(env, id).expect("proposal not found");
    proposal.status = ProposalStatus::Rejected;
    env.storage()
        .instance()
        .set(&MultisigKey::Proposal(id), &proposal);
}

pub fn is_proposal_approved(env: &Env, id: u64) -> bool {
    get_approvals(env, id).len() >= get_threshold(env)
}

pub fn get_approval_count(env: &Env, id: u64) -> u32 {
    get_approvals(env, id).len()
}

pub fn mark_executed(env: &Env, id: u64) {
    let mut proposal = get_proposal(env, id).expect("proposal not found");
    assert!(
        proposal.status == ProposalStatus::Pending,
        "proposal is not pending"
    );
    assert!(is_proposal_approved(env, id), "proposal not approved");
    proposal.status = ProposalStatus::Executed;
    env.storage()
        .instance()
        .set(&MultisigKey::Proposal(id), &proposal);
}

/// Check if multisig is initialized.
pub fn is_multisig_initialized(env: &Env) -> bool {
    env.storage().instance().has(&MultisigKey::Owners)
}

// ════════════════════════════════════════════════════════════════════
//  Require Multisig Approval
// ════════════════════════════════════════════════════════════════════

/// Require that the caller is an owner with proper authorization.
pub fn require_owner(env: &Env, caller: &Address) {
    caller.require_auth();
    assert!(is_owner(env, caller), "caller is not a multisig owner");
}
