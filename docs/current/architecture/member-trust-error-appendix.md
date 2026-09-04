# Member Execution Trust Error Appendix

This file is generated from `TrustErrorCode` in `crates/firm-core/src/agentfirm_api/work_trust.rs`. Do not edit it by hand. Run `node scripts/generate-member-trust-error-contract.mjs --write` after changing the Rust enum, then commit the schema and this appendix together.

Protocol: `agentfirm-member-trust/1`

| Code | Contract | Default retry guidance |
| --- | --- | --- |
| `VERSION_CONFLICT` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `IDEMPOTENCY_KEY_REUSED` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `UNAUTHORIZED_ACTOR` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `INVALID_STATE_TRANSITION` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `AGENT_MEMBER_PAUSED` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `AGENT_MEMBER_RETIRED` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `MEMBER_RUN_CLOSED` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `MEMBER_RUN_RETIRED` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `MEMBER_RUN_GENERATION_FENCED` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `SUPERVISOR_GENERATION_FENCED` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `NATIVE_SESSION_MISSING` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `NATIVE_SESSION_INCOMPATIBLE` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `DELIVERY_CLAIM_CONFLICT` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `DELIVERY_NOT_DISPATCHED` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `DELIVERY_RECEIPT_MISSING` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `DELIVERY_RECOVERY_UNCERTAIN` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `RUNTIME_EFFECT_UNKNOWN` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `WORK_REVISION_STALE` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `WORK_EXECUTION_BINDING_ACTIVE` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `WORKSPACE_PATH_UNSAFE` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `WORKSPACE_REPOSITORY_MISMATCH` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `WORKSPACE_LINK_ESCAPE` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `WORKSPACE_GENERATION_FENCED` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `WORKSPACE_DIRTY` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `WORKSPACE_CONFLICTED` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `WORKSPACE_CLEANUP_BLOCKED` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `MODULE_CONFIG_INVALID` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `MODULE_LIFECYCLE_VIOLATION` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `GATE_DEPENDENCY_CYCLE` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `GATE_REQUIREMENT_STALE` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `GATE_EVALUATION_REQUIRED` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `GATE_WAIVER_UNAUTHORIZED` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `REPORT_EVIDENCE_MISSING` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
| `FAILURE_ANALYSIS_MISSING` | Typed kernel rejection; inspect message, resource and current_version. | false unless the returned payload explicitly says otherwise |
