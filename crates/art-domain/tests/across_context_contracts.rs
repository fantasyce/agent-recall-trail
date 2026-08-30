use std::str::FromStr;

use art_domain::{
    ArtError,
    agent::AgentId,
    contracts::{AccessGrant, ContextPack, GrantUse, InvalidationEvent},
};
use chrono::{Duration, Utc};

fn grant() -> AccessGrant {
    AccessGrant {
        schema: "art.grant.v1".into(),
        grant_id: "artg_test".into(),
        owner_agent: AgentId::from_str("codex-primary").unwrap(),
        target_agent: AgentId::from_str("dsh-primary").unwrap(),
        purpose: "compare reviewed procedures".into(),
        source_refs: vec!["memory:artm_test@1".into()],
        allowed_fields: vec!["summary".into(), "verification".into()],
        expires_at: Utc::now() + Duration::minutes(5),
        max_uses: 1,
        no_persist: true,
        revocation_epoch: 3,
    }
}

#[test]
fn grant_is_purpose_target_field_ttl_and_use_bounded() {
    let grant = grant();
    let request = GrantUse {
        target_agent: AgentId::from_str("dsh-primary").unwrap(),
        purpose: "compare reviewed procedures".into(),
        requested_fields: vec!["summary".into()],
        uses_so_far: 0,
        observed_revocation_epoch: 3,
        now: Utc::now(),
    };
    assert!(grant.authorize(&request).is_ok());
    let mut wrong = request.clone();
    wrong.target_agent = AgentId::from_str("codex-secondary").unwrap();
    assert!(matches!(
        grant.authorize(&wrong),
        Err(ArtError::PermissionDenied(_))
    ));
    let mut expired = request;
    expired.now = grant.expires_at + Duration::seconds(1);
    assert!(matches!(
        grant.authorize(&expired),
        Err(ArtError::GrantExpired)
    ));

    let base = GrantUse {
        target_agent: AgentId::from_str("dsh-primary").unwrap(),
        purpose: "compare reviewed procedures".into(),
        requested_fields: vec!["summary".into()],
        uses_so_far: 0,
        observed_revocation_epoch: 3,
        now: Utc::now(),
    };
    let mut exhausted = base.clone();
    exhausted.uses_so_far = 1;
    assert!(matches!(
        grant.authorize(&exhausted),
        Err(ArtError::GrantExpired)
    ));
    let mut wrong_purpose = base.clone();
    wrong_purpose.purpose = "unrelated".into();
    assert!(matches!(
        grant.authorize(&wrong_purpose),
        Err(ArtError::PermissionDenied(_))
    ));
    let mut excessive_fields = base.clone();
    excessive_fields.requested_fields = vec!["private_payload".into()];
    assert!(matches!(
        grant.authorize(&excessive_fields),
        Err(ArtError::PermissionDenied(_))
    ));
    let mut stale_epoch = base;
    stale_epoch.observed_revocation_epoch = 2;
    assert!(matches!(
        grant.authorize(&stale_epoch),
        Err(ArtError::PermissionDenied(_))
    ));
    let mut persistable = grant;
    persistable.no_persist = false;
    assert!(matches!(
        persistable.authorize(&stale_epoch),
        Err(ArtError::InvalidInput(_))
    ));
}

#[test]
fn context_pack_has_no_persist_and_no_private_body_receipt() {
    let pack = ContextPack::from_grant(&grant(), vec!["safe excerpt".into()], Utc::now()).unwrap();
    assert!(pack.no_persist);
    assert_eq!(pack.delivery_receipt.subject_hashes.len(), 1);
    let serialized = serde_json::to_string(&pack.delivery_receipt).unwrap();
    assert!(!serialized.contains("safe excerpt"));
}

#[test]
fn invalidation_epoch_blocks_stale_consumers() {
    let event = InvalidationEvent {
        schema: "art.invalidation.v1".into(),
        subject_ref: "knowledge:arke_test".into(),
        new_epoch: 4,
        reason: "revoked".into(),
        occurred_at: Utc::now(),
    };
    assert!(event.validate_consumer_epoch(3).is_err());
    assert!(event.validate_consumer_epoch(4).is_ok());
}
