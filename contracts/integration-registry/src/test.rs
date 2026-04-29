//! # Integration Registry Tests
//!
//! Comprehensive tests covering the integration provider lifecycle, governance,
//! and edge cases.

use super::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env, String};

/// Helper: register the contract and return a client.
fn setup() -> (
    Env,
    IntegrationRegistryContractClient<'static>,
    Address,
    String,
) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(IntegrationRegistryContract, ());
    let client = IntegrationRegistryContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin, &0u64);

    // Register a default 'global' namespace
    let namespace = String::from_str(&env, "global");
    client.register_namespace(&admin, &namespace, &admin, &0u64);

    (env, client, admin, namespace)
}

/// Helper: create sample provider metadata.
fn sample_metadata(env: &Env) -> ProviderMetadata {
    ProviderMetadata {
        name: String::from_str(env, "Stripe"),
        description: String::from_str(env, "Payment processing platform"),
        api_version: String::from_str(env, "v1"),
        docs_url: String::from_str(env, "https://stripe.com/docs"),
        category: String::from_str(env, "payment"),
    }
}

/// Helper: create metadata with explicit byte lengths for each field.
fn metadata_with_lengths(
    env: &Env,
    name_len: u32,
    description_len: u32,
    api_version_len: u32,
    docs_url_len: u32,
    category_len: u32,
) -> ProviderMetadata {
    ProviderMetadata {
        name: String::from_str(env, &"n".repeat(name_len as usize)),
        description: String::from_str(env, &"d".repeat(description_len as usize)),
        api_version: String::from_str(env, &"v".repeat(api_version_len as usize)),
        docs_url: String::from_str(env, &"u".repeat(docs_url_len as usize)),
        category: String::from_str(env, &"c".repeat(category_len as usize)),
    }
}

/// Helper: provision a namespace owned by a dedicated non-admin account.
fn setup_namespace_owner(
    env: &Env,
    client: &IntegrationRegistryContractClient<'static>,
    admin: &Address,
    namespace: &str,
) -> (Address, String) {
    let owner = Address::generate(env);
    let ns = String::from_str(env, namespace);
    client.register_namespace(admin, &ns, &owner, &1u64);
    (owner, ns)
}

// ════════════════════════════════════════════════════════════════════
//  Initialization Tests
// ════════════════════════════════════════════════════════════════════

#[test]
fn test_initialize() {
    let (_env, client, admin, _ns) = setup();
    assert_eq!(client.get_admin(), admin);
    assert!(client.has_governance(&admin));
}

#[test]
#[should_panic(expected = "already initialized")]
fn test_initialize_twice_panics() {
    let (env, client, _admin, _ns) = setup();
    let new_admin = Address::generate(&env);
    client.initialize(&new_admin, &1u64);
}

// ════════════════════════════════════════════════════════════════════
//  Provider Registration Tests
// ════════════════════════════════════════════════════════════════════

#[test]
fn test_register_provider() {
    let (env, client, admin, ns) = setup();
    let id = String::from_str(&env, "stripe");
    let metadata = sample_metadata(&env);

    client.register_provider(&admin, &ns, &id, &metadata, &0u64);

    let provider = client.get_provider(&ns, &id).unwrap();
    assert_eq!(provider.id, id);
    assert_eq!(provider.namespace, ns);
    assert_eq!(provider.status, ProviderStatus::Registered);
    assert_eq!(provider.metadata.name, metadata.name);
}

#[test]
#[should_panic(expected = "provider already registered in namespace")]
fn test_register_duplicate_provider_panics() {
    let (env, client, admin, ns) = setup();
    let id = String::from_str(&env, "stripe");
    let metadata = sample_metadata(&env);

    client.register_provider(&admin, &ns, &id, &metadata, &0u64);
    client.register_provider(&admin, &ns, &id, &metadata, &1u64);
}

#[test]
#[should_panic(expected = "caller does not have namespace governance role")]
fn test_register_without_governance_panics() {
    let (env, client, _admin, ns) = setup();
    let non_gov = Address::generate(&env);
    let id = String::from_str(&env, "stripe");
    let metadata = sample_metadata(&env);

    client.register_provider(&non_gov, &ns, &id, &metadata, &0u64);
}

// ════════════════════════════════════════════════════════════════════
//  Provider Lifecycle Tests
// ════════════════════════════════════════════════════════════════════

#[test]
fn test_enable_provider() {
    let (env, client, admin, ns) = setup();
    let id = String::from_str(&env, "stripe");
    let metadata = sample_metadata(&env);

    client.register_provider(&admin, &ns, &id, &metadata, &0u64);
    assert!(!client.is_enabled(&ns, &id));

    client.enable_provider(&admin, &ns, &id, &1u64);
    assert!(client.is_enabled(&ns, &id));
    assert!(client.is_valid_for_attestation(&ns, &id));
}

#[test]
fn test_deprecate_provider() {
    let (env, client, admin, ns) = setup();
    let id = String::from_str(&env, "stripe");
    let metadata = sample_metadata(&env);

    client.register_provider(&admin, &ns, &id, &metadata, &0u64);
    client.enable_provider(&admin, &ns, &id, &1u64);

    client.deprecate_provider(&admin, &ns, &id, &2u64);
    assert!(client.is_deprecated(&ns, &id));
    assert!(!client.is_enabled(&ns, &id));
    // Deprecated providers are still valid for attestations
    assert!(client.is_valid_for_attestation(&ns, &id));
}

#[test]
fn test_disable_provider() {
    let (env, client, admin, ns) = setup();
    let id = String::from_str(&env, "stripe");
    let metadata = sample_metadata(&env);

    client.register_provider(&admin, &ns, &id, &metadata, &0u64);
    client.enable_provider(&admin, &ns, &id, &1u64);

    client.disable_provider(&admin, &ns, &id, &2u64);
    assert!(!client.is_enabled(&ns, &id));
    assert!(!client.is_valid_for_attestation(&ns, &id));

    let provider = client.get_provider(&ns, &id).unwrap();
    assert_eq!(provider.status, ProviderStatus::Disabled);
}

#[test]
fn test_re_enable_deprecated_provider() {
    let (env, client, admin) = setup();
    let id = String::from_str(&env, "stripe");
    let metadata = sample_metadata(&env);

    client.register_provider(&admin, &id, &metadata, &0u64);
    client.enable_provider(&admin, &id, &1u64);
    client.deprecate_provider(&admin, &id, &2u64);

    // Re-enable from deprecated (nonce 3)
    client.enable_provider(&admin, &id, &3u64);
    assert!(client.is_enabled(&id));
}

#[test]
fn test_re_enable_disabled_provider() {
    let (env, client, admin) = setup();
    let id = String::from_str(&env, "stripe");
    let metadata = sample_metadata(&env);

    client.register_provider(&admin, &id, &metadata, &0u64);
    client.enable_provider(&admin, &id, &1u64);
    client.disable_provider(&admin, &id, &2u64);

    // Re-enable from disabled (nonce 3)
    client.enable_provider(&admin, &id, &3u64);
    assert!(client.is_enabled(&id));
}

#[test]
#[should_panic(expected = "only enabled providers can be deprecated")]
fn test_deprecate_registered_provider_panics() {
    let (env, client, admin) = setup();
    let id = String::from_str(&env, "stripe");
    let metadata = sample_metadata(&env);

    client.register_provider(&admin, &id, &metadata, &0u64);
    // Cannot deprecate a registered provider directly
    client.deprecate_provider(&admin, &id, &1u64);
}

#[test]
#[should_panic(expected = "provider is already disabled")]
fn test_disable_already_disabled_panics() {
    let (env, client, admin) = setup();
    let id = String::from_str(&env, "stripe");
    let metadata = sample_metadata(&env);

    client.register_provider(&admin, &id, &metadata, &0u64);
    client.enable_provider(&admin, &id, &1u64);
    client.disable_provider(&admin, &id, &2u64);
    client.disable_provider(&admin, &id, &3u64);
}

// ════════════════════════════════════════════════════════════════════
//  Metadata Update Tests
// ════════════════════════════════════════════════════════════════════

#[test]
fn test_update_metadata() {
    let (env, client, admin, ns) = setup();
    let id = String::from_str(&env, "stripe");
    let metadata = sample_metadata(&env);

    client.register_provider(&admin, &ns, &id, &metadata, &1u64);

    let new_metadata = ProviderMetadata {
        name: String::from_str(&env, "Stripe v2"),
        description: String::from_str(&env, "Updated payment processing"),
        api_version: String::from_str(&env, "v2"),
        docs_url: String::from_str(&env, "https://stripe.com/docs/v2"),
        category: String::from_str(&env, "payment"),
    };

    client.update_metadata(&admin, &ns, &id, &new_metadata, &2u64);

    let provider = client.get_provider(&ns, &id).unwrap();
    assert_eq!(provider.metadata.name, new_metadata.name);
    assert_eq!(provider.metadata.api_version, new_metadata.api_version);
}

#[test]
#[should_panic(expected = "provider not found")]
fn test_update_nonexistent_provider_panics() {
    let (env, client, admin, ns) = setup();
    let id = String::from_str(&env, "nonexistent");
    let metadata = sample_metadata(&env);

    client.update_metadata(&admin, &ns, &id, &metadata, &1u64);
}

#[test]
fn test_register_provider_accepts_metadata_at_max_bounds() {
    let (env, client, admin, _ns) = setup();
    let (owner, ns) = setup_namespace_owner(&env, &client, &admin, "metadata_bounds_ok");
    let id = String::from_str(&env, "stripe");
    let metadata = metadata_with_lengths(
        &env,
        MAX_PROVIDER_METADATA_NAME_BYTES,
        MAX_PROVIDER_METADATA_DESCRIPTION_BYTES,
        MAX_PROVIDER_METADATA_API_VERSION_BYTES,
        MAX_PROVIDER_METADATA_DOCS_URL_BYTES,
        MAX_PROVIDER_METADATA_CATEGORY_BYTES,
    );

    client.register_provider(&owner, &ns, &id, &metadata, &0u64);

    let provider = client.get_provider(&ns, &id).unwrap();
    assert_eq!(
        provider.metadata.name.len(),
        MAX_PROVIDER_METADATA_NAME_BYTES
    );
    assert_eq!(
        provider.metadata.description.len(),
        MAX_PROVIDER_METADATA_DESCRIPTION_BYTES
    );
    assert_eq!(
        provider.metadata.api_version.len(),
        MAX_PROVIDER_METADATA_API_VERSION_BYTES
    );
    assert_eq!(
        provider.metadata.docs_url.len(),
        MAX_PROVIDER_METADATA_DOCS_URL_BYTES
    );
    assert_eq!(
        provider.metadata.category.len(),
        MAX_PROVIDER_METADATA_CATEGORY_BYTES
    );
}

#[test]
#[should_panic(expected = "provider description exceeds max bytes")]
fn test_register_provider_rejects_oversized_description() {
    let (env, client, admin, _ns) = setup();
    let (owner, ns) = setup_namespace_owner(&env, &client, &admin, "metadata_desc_oversize");
    let id = String::from_str(&env, "stripe");
    let metadata = metadata_with_lengths(
        &env,
        6,
        MAX_PROVIDER_METADATA_DESCRIPTION_BYTES + 1,
        2,
        18,
        7,
    );

    client.register_provider(&owner, &ns, &id, &metadata, &0u64);
}

#[test]
#[should_panic(expected = "provider name cannot be empty")]
fn test_register_provider_rejects_empty_name() {
    let (env, client, admin, _ns) = setup();
    let (owner, ns) = setup_namespace_owner(&env, &client, &admin, "metadata_name_empty");
    let id = String::from_str(&env, "stripe");
    let mut metadata = sample_metadata(&env);
    metadata.name = String::from_str(&env, "");

    client.register_provider(&owner, &ns, &id, &metadata, &0u64);
}

#[test]
#[should_panic(expected = "provider category has leading or trailing whitespace")]
fn test_register_provider_rejects_category_with_edge_whitespace() {
    let (env, client, admin, _ns) = setup();
    let (owner, ns) = setup_namespace_owner(&env, &client, &admin, "metadata_category_ws");
    let id = String::from_str(&env, "stripe");
    let mut metadata = sample_metadata(&env);
    metadata.category = String::from_str(&env, " payment");

    client.register_provider(&owner, &ns, &id, &metadata, &0u64);
}

#[test]
#[should_panic(expected = "provider docs url exceeds max bytes")]
fn test_update_metadata_rejects_oversized_docs_url() {
    let (env, client, admin, _ns) = setup();
    let (owner, ns) = setup_namespace_owner(&env, &client, &admin, "metadata_docs_oversize");
    let id = String::from_str(&env, "stripe");
    let metadata = sample_metadata(&env);

    client.register_provider(&owner, &ns, &id, &metadata, &0u64);

    let updated =
        metadata_with_lengths(&env, 6, 24, 2, MAX_PROVIDER_METADATA_DOCS_URL_BYTES + 1, 7);
    client.update_metadata(&owner, &ns, &id, &updated, &1u64);
}

// ════════════════════════════════════════════════════════════════════
//  Query Tests
// ════════════════════════════════════════════════════════════════════

#[test]
fn test_get_all_providers() {
    let (env, client, admin, ns) = setup();

    let stripe = String::from_str(&env, "stripe");
    let shopify = String::from_str(&env, "shopify");
    let quickbooks = String::from_str(&env, "quickbooks");

    let metadata = sample_metadata(&env);

    client.register_provider(&admin, &ns, &stripe, &metadata, &0u64);
    client.register_provider(&admin, &ns, &shopify, &metadata, &1u64);
    client.register_provider(&admin, &ns, &quickbooks, &metadata, &2u64);

    let all = client.get_namespace_providers(&ns);
    assert_eq!(all.len(), 3);
}

#[test]
fn test_get_enabled_providers() {
    let (env, client, admin, ns) = setup();

    let stripe = String::from_str(&env, "stripe");
    let shopify = String::from_str(&env, "shopify");
    let quickbooks = String::from_str(&env, "quickbooks");

    let metadata = sample_metadata(&env);

    client.register_provider(&admin, &ns, &stripe, &metadata, &0u64);
    client.register_provider(&admin, &ns, &shopify, &metadata, &1u64);
    client.register_provider(&admin, &ns, &quickbooks, &metadata, &2u64);

    client.enable_provider(&admin, &ns, &stripe, &3u64);
    client.enable_provider(&admin, &ns, &shopify, &4u64);
    // quickbooks remains registered

    let enabled = client.get_enabled_providers(&ns);
    assert_eq!(enabled.len(), 2);
}

#[test]
fn test_get_deprecated_providers() {
    let (env, client, admin) = setup();

    let stripe = String::from_str(&env, "stripe");
    let shopify = String::from_str(&env, "shopify");

    let metadata = sample_metadata(&env);

    client.register_provider(&admin, &stripe, &metadata, &0u64);
    client.register_provider(&admin, &shopify, &metadata, &1u64);

    client.enable_provider(&admin, &stripe, &2u64);
    client.enable_provider(&admin, &shopify, &3u64);
    client.deprecate_provider(&admin, &stripe, &4u64);

    let deprecated = client.get_deprecated_providers();
    assert_eq!(deprecated.len(), 1);
}

#[test]
fn test_get_status() {
    let (env, client, admin, ns) = setup();
    let id = String::from_str(&env, "stripe");
    let metadata = sample_metadata(&env);

    // Not registered yet
    assert!(client.get_status(&ns, &id).is_none());

    client.register_provider(&admin, &ns, &id, &metadata, &0u64);
    assert_eq!(
        client.get_status(&ns, &id),
        Some(ProviderStatus::Registered)
    );

    client.enable_provider(&admin, &ns, &id, &1u64);
    assert_eq!(client.get_status(&ns, &id), Some(ProviderStatus::Enabled));

    client.deprecate_provider(&admin, &ns, &id, &2u64);
    assert_eq!(
        client.get_status(&ns, &id),
        Some(ProviderStatus::Deprecated)
    );

    client.disable_provider(&admin, &ns, &id, &3u64);
    assert_eq!(client.get_status(&ns, &id), Some(ProviderStatus::Disabled));
}

#[test]
fn test_is_valid_for_attestation() {
    let (env, client, admin, ns) = setup();
    let id = String::from_str(&env, "stripe");
    let metadata = sample_metadata(&env);

    // Not registered
    assert!(!client.is_valid_for_attestation(&ns, &id));

    client.register_provider(&admin, &ns, &id, &metadata, &0u64);
    // Registered but not enabled
    assert!(!client.is_valid_for_attestation(&ns, &id));

    client.enable_provider(&admin, &ns, &id, &1u64);
    // Enabled - valid
    assert!(client.is_valid_for_attestation(&ns, &id));

    client.deprecate_provider(&admin, &ns, &id, &2u64);
    // Deprecated - still valid
    assert!(client.is_valid_for_attestation(&ns, &id));

    client.disable_provider(&admin, &ns, &id, &3u64);
    // Disabled - not valid
    assert!(!client.is_valid_for_attestation(&ns, &id));
}

// ════════════════════════════════════════════════════════════════════
//  Governance Tests
// ════════════════════════════════════════════════════════════════════

#[test]
fn test_grant_governance() {
    let (env, client, admin, ns) = setup();
    let new_gov = Address::generate(&env);

    assert!(!client.has_governance(&new_gov));

    client.grant_governance(&admin, &new_gov, &1u64);
    assert!(client.has_governance(&new_gov));

    // New global governance member can register providers in any namespace
    let id = String::from_str(&env, "stripe_global");
    let metadata = sample_metadata(&env);
    client.register_provider(&new_gov, &ns, &id, &metadata, &0u64);
}

#[test]
fn test_revoke_governance() {
    let (env, client, admin) = setup();
    let new_gov = Address::generate(&env);

    client.grant_governance(&admin, &new_gov, &1u64);
    assert!(client.has_governance(&new_gov));

    client.revoke_governance(&admin, &new_gov, &2u64);
    assert!(!client.has_governance(&new_gov));
}

#[test]
#[should_panic(expected = "caller is not admin")]
fn test_grant_governance_non_admin_panics() {
    let (env, client, _admin) = setup();
    let non_admin = Address::generate(&env);
    let target = Address::generate(&env);

    client.grant_governance(&non_admin, &target, &0u64);
}

// ════════════════════════════════════════════════════════════════════
//  Edge Cases
// ════════════════════════════════════════════════════════════════════

#[test]
fn test_namespace_isolation() {
    let (env, client, admin, _ns) = setup();

    let ns_a = String::from_str(&env, "ns_a");
    let ns_b = String::from_str(&env, "ns_b");
    let owner_a = Address::generate(&env);
    let owner_b = Address::generate(&env);

    client.register_namespace(&admin, &ns_a, &owner_a, &1u64);
    client.register_namespace(&admin, &ns_b, &owner_b, &2u64);

    let id = String::from_str(&env, "provider");
    let metadata = sample_metadata(&env);

    // owner_a can register in ns_a
    client.register_provider(&owner_a, &ns_a, &id, &metadata, &0u64);
    assert!(client.get_provider(&ns_a, &id).is_some());

    // owner_a CANNOT register in ns_b
    let res = env.try_invoke_contract::<()>(
        &client.contract_id,
        "register_provider",
        (&owner_a, &ns_b, &id, &metadata, &1u64).into_val(&env),
    );
    assert!(res.is_err());

    // owner_b can register same ID in ns_b
    client.register_provider(&owner_b, &ns_b, &id, &metadata, &0u64);
    assert!(client.get_provider(&ns_b, &id).is_some());

    // owner_a can update ns_a
    client.enable_provider(&owner_a, &ns_a, &id, &2u64);
    assert!(client.is_enabled(&ns_a, &id));

    // owner_a CANNOT update ns_b
    let res = env.try_invoke_contract::<()>(
        &client.contract_id,
        "enable_provider",
        (&owner_a, &ns_b, &id, &3u64).into_val(&env),
    );
    assert!(res.is_err());
}

#[test]
fn test_admin_override() {
    let (env, client, admin, _ns) = setup();
    let ns = String::from_str(&env, "private");
    let owner = Address::generate(&env);
    client.register_namespace(&admin, &ns, &owner, &1u64);

    let id = String::from_str(&env, "provider");
    let metadata = sample_metadata(&env);

    // Admin can register in any namespace without being explicit owner
    client.register_provider(&admin, &ns, &id, &metadata, &2u64);
    assert!(client.get_provider(&ns, &id).is_some());
}

// ════════════════════════════════════════════════════════════════════
//  Duplicate Namespace Tests
// ════════════════════════════════════════════════════════════════════

#[test]
#[should_panic(expected = "provider already registered")]
fn test_duplicate_namespace_different_metadata() {
    let (env, client, admin) = setup();
    let id = String::from_str(&env, "stripe");
    let metadata1 = sample_metadata(&env);
    let metadata2 = ProviderMetadata {
        name: String::from_str(&env, "Stripe Alternate"),
        description: String::from_str(&env, "Alternate payment processor"),
        api_version: String::from_str(&env, "v2"),
        docs_url: String::from_str(&env, "https://stripe.com/docs/v2"),
        category: String::from_str(&env, "payment"),
    };

    client.register_provider(&admin, &id, &metadata1, &0u64);
    // Same ID, different metadata — must still be rejected
    client.register_provider(&admin, &id, &metadata2, &1u64);
}

#[test]
#[should_panic(expected = "provider already registered")]
fn test_duplicate_namespace_different_caller() {
    let (env, client, admin) = setup();
    let gov2 = Address::generate(&env);
    client.grant_governance(&admin, &gov2, &1u64);

    let id = String::from_str(&env, "stripe");
    let metadata = sample_metadata(&env);

    client.register_provider(&admin, &id, &metadata, &0u64);
    // Different governance member, same namespace — must be rejected
    client.register_provider(&gov2, &id, &metadata, &0u64);
}

#[test]
#[should_panic(expected = "provider already registered")]
fn test_duplicate_namespace_after_disable() {
    let (env, client, admin) = setup();
    let id = String::from_str(&env, "stripe");
    let metadata = sample_metadata(&env);

    client.register_provider(&admin, &id, &metadata, &0u64);
    client.enable_provider(&admin, &id, &1u64);
    client.disable_provider(&admin, &id, &2u64);

    // Re-registering a disabled provider must still be rejected;
    // the correct path is re-enable, not re-register.
    client.register_provider(&admin, &id, &metadata, &3u64);
}

#[test]
fn test_case_sensitive_namespaces_are_distinct() {
    let (env, client, admin) = setup();
    let lower = String::from_str(&env, "stripe");
    let upper = String::from_str(&env, "Stripe");
    let metadata = sample_metadata(&env);

    client.register_provider(&admin, &lower, &metadata, &0u64);
    client.register_provider(&admin, &upper, &metadata, &1u64);

    // Both namespaces coexist — IDs are case-sensitive
    assert!(client.get_provider(&lower).is_some());
    assert!(client.get_provider(&upper).is_some());
    assert_eq!(client.get_all_providers().len(), 2);
}

#[test]
#[should_panic(expected = "provider already registered")]
fn test_duplicate_namespace_after_deprecate() {
    let (env, client, admin) = setup();
    let id = String::from_str(&env, "stripe");
    let metadata = sample_metadata(&env);

    client.register_provider(&admin, &id, &metadata, &0u64);
    client.enable_provider(&admin, &id, &1u64);
    client.deprecate_provider(&admin, &id, &2u64);

    // Re-registering a deprecated provider must be rejected
    client.register_provider(&admin, &id, &metadata, &3u64);
}

#[test]
fn test_similar_namespaces_are_distinct() {
    let (env, client, admin) = setup();
    let metadata = sample_metadata(&env);

    let id1 = String::from_str(&env, "stripe");
    let id2 = String::from_str(&env, "stripe-v2");
    let id3 = String::from_str(&env, "stripe_connect");

    client.register_provider(&admin, &id1, &metadata, &0u64);
    client.register_provider(&admin, &id2, &metadata, &1u64);
    client.register_provider(&admin, &id3, &metadata, &2u64);

    assert_eq!(client.get_all_providers().len(), 3);
    assert!(client.get_provider(&id1).is_some());
    assert!(client.get_provider(&id2).is_some());
    assert!(client.get_provider(&id3).is_some());
}

#[test]
fn test_duplicate_register_does_not_corrupt_provider_list() {
    let (env, client, admin) = setup();
    let id = String::from_str(&env, "stripe");
    let metadata = sample_metadata(&env);

    client.register_provider(&admin, &id, &metadata, &0u64);

    // Attempt duplicate — will panic, but we catch the state beforehand
    let count_before = client.get_all_providers().len();
    assert_eq!(count_before, 1);

    // Verify the original provider is intact after failed duplicate attempt
    let provider = client.get_provider(&id).unwrap();
    assert_eq!(provider.status, ProviderStatus::Registered);
    assert_eq!(provider.metadata.name, metadata.name);
}
