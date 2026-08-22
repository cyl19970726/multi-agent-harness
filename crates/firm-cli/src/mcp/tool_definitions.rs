//! MCP tool schemas advertised by `tools/list`.

use super::MCP_MEMBER_TRUST_COMMANDS;
use serde_json::{json, Value};

/// Mission / Mission Log authoring plus Agent Team tools. Descriptions are the
/// interface contract the host model reads when deciding how to call a tool.
pub(super) fn tool_definitions() -> Value {
    json!([
        {
            "name": "agentfirm_member_trust_mutate",
            "description": "Execute one advertised Member Execution Trust lifecycle or Work command. MemberRun creation is available only through team_run_create or team_run_add_member. Close requires Active; Reopen requires Closed; ResumeNativeSession requires Active plus a Disconnected, Failed, or Stopped runtime. Lifecycle changes use the combined TeamRun authority and never mutate only one projection. Actor identity comes only from the MCP process transport environment.",
            "inputSchema": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "command": {
                        "type": "object",
                        "description": "One tagged command from the closed MCP Member Trust inventory.",
                        "properties": {
                            "command": {"type": "string", "enum": MCP_MEMBER_TRUST_COMMANDS}
                        },
                        "required": ["command"]
                    },
                    "idempotency_key": {"type": "string", "minLength": 1},
                    "expected_version": {"type": "integer", "minimum": 0}
                },
                "required": ["command", "idempotency_key", "expected_version"]
            }
        },
        {
            "name": "remote_fabric_status",
            "description": "Read current Node-local Remote Fabric queue/session truth and, when this machine hosts it, Company Control Plane Node/lease diagnostics. This tool is read-only and never reconstructs route truth from Message or runtime ledgers.",
            "inputSchema": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "company_id": {"type": "string", "minLength": 1}
                },
                "required": ["company_id"]
            }
        },
        {
            "name": "remote_fabric_operation_show",
            "description": "Read one RoutedOperation with its transport Attempts and generation-fenced Receipts from the local Company Control Plane FabricStore. It is unavailable away from the Control Plane and performs no replay or mutation.",
            "inputSchema": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "company_id": {"type": "string", "minLength": 1},
                    "operation_id": {"type": "string", "minLength": 1}
                },
                "required": ["company_id", "operation_id"]
            }
        },
        {
            "name": "mission_list",
            "description": "Read-only legacy read of historical Mission rows (DOC-108). Not current authority; no writer surface remains.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "team_run_create",
            "description": "Create one runtime attempt from a required flat AgentTeam. ExecutionNode and Project Binding are derived from the durable Team and selected execution context; members can come from the Team definition. Legacy Mission provenance is optional and never required.",
            "inputSchema": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "objective": {"type": "string", "minLength": 1, "description": "Durable TeamRun context. It never silently assigns the same responsibility to every member."},
                    "budget_limit_usd": {"type": "number", "minimum": 0, "description": "Optional budget cap in USD, recorded on the run."},
                    "previous_run_id": {"type": "string", "description": "Optional previous attempt id; it must belong to the same durable AgentTeam."},
                    "agent_team_id": {"type": "string", "minLength": 1, "description": "Required durable flat AgentTeam identity."},
                    "execution_root": {"type": "string", "minLength": 1, "description": "Optional TeamRun execution root. Must be the selected project_root or a Git worktree sharing its git common directory; defaults to project_root."},
                    "host_surface": {"type": "string", "minLength": 1, "description": "Exact provider-native Host surface, for example codex-app. Defaults to mcp when the calling Host does not bind itself."},
                    "host_thread_id": {"type": "string", "minLength": 1, "description": "Exact native Host task/session id. Required for Plugin safe-boundary delivery to this Host."},
                    "host_runtime_mode": {"type": "string", "enum": ["managed", "external_interactive"], "default": "managed", "description": "Managed is the default and runs the Host through the same MemberRun/AgentSession/NodeDaemon path as Members. external_interactive is user-driven and pull-only; host_thread_id is external-only."},
                    "members": {
                        "type": "array",
                        "description": "One entry per team member.",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": {"type": "string", "minLength": 1, "description": "Member display name, unique within the run."},
                                "agent_member_id": {"type": "string", "minLength": 1, "description": "Exact durable AgentMember identity, including the Host AgentMember."},
                                "role": {"type": "string", "minLength": 1, "description": "e.g. coordinator / implementer / reviewer."},
                                "provider": {"type": "string", "minLength": 1, "description": "Provider label. Harness-driven modes require a registered codex, claude, kimi, or pi adapter; external_interactive accepts any non-empty label because Harness does not execute it."},
                                "execution_mode": {"type": "string", "enum": ["codex_app_server", "claude_agent_sdk", "kimi_acp", "pi_rpc", "external_interactive"], "description": "Optional provider-specific Agent Team mode. Current bindings are codex_app_server, claude_agent_sdk, kimi_acp, and pi_rpc; retired one-shot modes such as codex_exec and claude_cli are rejected. external_interactive declares the user's own already-open session: Harness spawns no provider process, does not constrain its provider label, and the member polls its own inbox."},
                                "model": {"type": "string", "minLength": 1, "description": "Optional provider model override."},
                                "effort": {"type": "string", "minLength": 1, "description": "Optional provider-neutral reasoning-effort request. The adapter must record the provider-confirmed effective value or an unsupported/review_required status."},
                                "service_tier": {"type": "string", "minLength": 1, "description": "Optional provider-neutral latency/service profile request, such as priority. This is not a universal fast boolean."},
                                "provider_cwd_hint": {"type": "string", "minLength": 1, "description": "Optional member workspace override. Must be the selected project_root or a Git worktree sharing its git common directory. A managed Kimi Host requires an explicit Host worktree distinct from the Team execution root and not reserved by another active MemberRun."},
                                "owned_paths": {"type": "array", "items": {"type": "string", "minLength": 1}, "description": "Paths this member exclusively owns."},
                                "initial_work": {"type": "string", "minLength": 1, "description": "Optional completion criteria for one initial Host-assigned Work. Omit to create the member idle."},
                                "resume_native_session_id": {"type": "string", "minLength": 1, "description": "Explicit provider-owned session to resume. Never inferred from recent local history."}
                            },
                            "required": ["agent_member_id", "name", "role", "provider"]
                        }
                    }
                },
                "required": ["objective", "agent_team_id"]
            }
        },
        {
            "name": "team_run_work_list",
            "description": "List the authoritative shared Works board for one TeamRun. brief=true returns compact works_brief text lines (id, status, owner, version, title<=60 chars) instead of full Work JSON. since=<cursor> is a delta read: only Works whose latest WorkOperation postdates the cursor (a WorkOperation-order sequence, not a Work version), returned alongside a next_since watermark to chain the next call; combine with brief for the smallest board read.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string", "minLength": 1},
                    "brief": {"type": "boolean", "default": false, "description": "Return works_brief (one compact text line per Work) instead of works (full Work JSON)."},
                    "since": {"type": "integer", "minimum": 0, "description": "WorkOperation-order cursor. Only Works that changed after this point are returned; response adds since/next_since."}
                },
                "required": ["team_run_id"]
            }
        },
        {
            "name": "team_run_work_show",
            "description": "Show one Work with its append-only WorkEvents and latest WorkDeliveries.",
            "inputSchema": {"type": "object", "properties": {"team_run_id": {"type": "string"}, "work_id": {"type": "string"}}, "required": ["team_run_id", "work_id"]}
        },
        {
            "name": "team_run_work_create",
            "description": "Create durable team responsibility. Host may assign it immediately, expose it for self-claim, or leave it unassigned.",
            "inputSchema": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "team_run_id": {"type": "string", "minLength": 1},
                    "id": {"type": "string", "minLength": 1, "description": "Optional caller-stable Work id."},
                    "title": {"type": "string", "minLength": 1},
                    "context_markdown": {"type": "string"},
                    "completion_criteria_markdown": {"type": "string", "minLength": 1},
                    "owner_member_run_id": {"type": "string", "minLength": 1, "description": "Optional concrete ProviderRuntimeProjection to receive the first ProviderWorkDispatch; stable AgentMember ownership is derived by the store."},
                    "claim_mode": {"type": "string", "enum": ["host_assign", "team_claim"]},
                    "eligible_member_ids": {"type": "array", "items": {"type": "string", "minLength": 1}},
                    "parent_work_id": {"type": "string", "minLength": 1},
                    "prerequisite_work_ids": {"type": "array", "items": {"type": "string", "minLength": 1}},
                    "priority": {"type": "string", "enum": ["low", "normal", "high", "urgent"]},
                    "caused_by_message_id": {"type": "string", "minLength": 1},
                    "idempotency_key": {"type": "string", "minLength": 1}
                },
                "required": ["team_run_id", "title", "completion_criteria_markdown"]
            }
        },
        {
            "name": "team_run_work_assign",
            "description": "Host performs the first assignment of open Work using optimistic versioning. This does not move an existing stable owner to another runtime; use team_run_work_rebind for that.",
            "inputSchema": {"type": "object", "properties": {"team_run_id": {"type": "string"}, "work_id": {"type": "string"}, "member_run_id": {"type": "string"}, "expected_version": {"type": "integer", "minimum": 0}, "caused_by_message_id": {"type": "string"}, "idempotency_key": {"type": "string"}}, "required": ["team_run_id", "work_id", "member_run_id", "expected_version"]}
        },
        {
            "name": "team_run_work_rebind",
            "description": "Host preserves the Work's stable AgentMember owner while moving its active runtime binding to another active ProviderRuntimeProjection for that same identity, for example after a runtime replacement or crash recovery.",
            "inputSchema": {"type": "object", "properties": {"team_run_id": {"type": "string"}, "work_id": {"type": "string"}, "member_run_id": {"type": "string"}, "expected_version": {"type": "integer", "minimum": 0}, "caused_by_message_id": {"type": "string"}, "idempotency_key": {"type": "string"}}, "required": ["team_run_id", "work_id", "member_run_id", "expected_version"]}
        },
        {
            "name": "team_run_work_block",
            "description": "Host pauses owned in-progress Work with a durable blocker reason. Use ordinary Work-linked messages only for the surrounding discussion.",
            "inputSchema": {"type": "object", "properties": {"team_run_id": {"type": "string"}, "work_id": {"type": "string"}, "expected_version": {"type": "integer", "minimum": 0}, "reason": {"type": "string", "minLength": 1}, "caused_by_message_id": {"type": "string"}, "idempotency_key": {"type": "string"}}, "required": ["team_run_id", "work_id", "expected_version", "reason"]}
        },
        {
            "name": "team_run_work_resume",
            "description": "Host resumes blocked Work after recording how the blocker was resolved; the latest owner is woken through ProviderWorkDispatch.",
            "inputSchema": {"type": "object", "properties": {"team_run_id": {"type": "string"}, "work_id": {"type": "string"}, "expected_version": {"type": "integer", "minimum": 0}, "resolution": {"type": "string", "minLength": 1}, "caused_by_message_id": {"type": "string"}, "idempotency_key": {"type": "string"}}, "required": ["team_run_id", "work_id", "expected_version", "resolution"]}
        },
        {
            "name": "team_run_work_release",
            "description": "Host releases open owned Work back to the shared Ready Pool when it has not been claimed or delivered to a provider.",
            "inputSchema": {"type": "object", "properties": {"team_run_id": {"type": "string"}, "work_id": {"type": "string"}, "expected_version": {"type": "integer", "minimum": 0}, "caused_by_message_id": {"type": "string"}, "idempotency_key": {"type": "string"}}, "required": ["team_run_id", "work_id", "expected_version"]}
        },
        {
            "name": "team_run_work_request_changes",
            "description": "Host returns submitted Work with specific feedback; a new delivery wakes the current owner.",
            "inputSchema": {"type": "object", "properties": {"team_run_id": {"type": "string"}, "work_id": {"type": "string"}, "expected_version": {"type": "integer", "minimum": 0}, "reason": {"type": "string", "minLength": 1}, "caused_by_message_id": {"type": "string"}, "idempotency_key": {"type": "string"}}, "required": ["team_run_id", "work_id", "expected_version", "reason"]}
        },
        {
            "name": "team_run_work_cancel",
            "description": "Host cancels unfinished Work without closing the member or TeamRun.",
            "inputSchema": {"type": "object", "properties": {"team_run_id": {"type": "string"}, "work_id": {"type": "string"}, "expected_version": {"type": "integer", "minimum": 0}, "reason": {"type": "string", "minLength": 1}, "caused_by_message_id": {"type": "string"}, "idempotency_key": {"type": "string"}}, "required": ["team_run_id", "work_id", "expected_version", "reason"]}
        },
        {
            "name": "team_run_work_reconcile_delivery",
            "description": "A successor Supervisor explicitly requeues one stale claimed ProviderWorkDispatch after a crash. The caller must name the successor Supervisor id and generation; this never guesses provider consumption or changes Work responsibility.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "delivery_id": {"type": "string"},
                    "supervisor_id": {"type": "string"},
                    "supervisor_generation": {"type": "integer", "minimum": 1}
                },
                "required": ["team_run_id", "delivery_id", "supervisor_id", "supervisor_generation"]
            }
        },
        {
            "name": "collaboration_delegation_list",
            "description": "Read the Company Control Plane's canonical cross-Team Delegations. This tool never folds an Execution Space's retired local WorkDelegation ledger.",
            "inputSchema": {"type": "object", "additionalProperties": false, "properties": {"source_team_id": {"type": "string", "minLength": 1}, "target_team_id": {"type": "string", "minLength": 1}, "node_id": {"type": "string", "minLength": 1}, "state": {"type": "string", "enum": ["proposed", "awaiting_target_decision", "provisioning_target_work", "active", "result_available", "cancellation_requested", "terminal"]}, "limit": {"type": "integer", "minimum": 1, "maximum": 100}, "cursor": {"type": "string", "minLength": 1}}}
        },
        {
            "name": "collaboration_delegation_show",
            "description": "Read one canonical Company WorkDelegation with its cancellation requests and immutable remote publications.",
            "inputSchema": {"type": "object", "additionalProperties": false, "properties": {"delegation_id": {"type": "string", "minLength": 1}}, "required": ["delegation_id"]}
        },
        {
            "name": "execution_node_list",
            "description": "List stable ExecutionNodes with project registrations and current NodeDaemon lease generations.",
            "inputSchema": {"type": "object", "additionalProperties": false, "properties": {}}
        },
        {
            "name": "execution_node_show",
            "description": "Show one ExecutionNode with its registrations and current NodeDaemon lease.",
            "inputSchema": {"type": "object", "additionalProperties": false, "properties": {"node_id": {"type": "string", "minLength": 1}}, "required": ["node_id"]}
        },
        {
            "name": "team_run_add_member",
            "description": "Add one idle member to an active planning/running/waiting TeamRun and optionally create a first Work.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "initial_work": {"type": "string", "minLength": 1},
                    "member": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string", "minLength": 1},
                            "role": {"type": "string", "minLength": 1},
                            "provider": {"type": "string", "minLength": 1},
                            "execution_mode": {"type": "string", "enum": ["codex_app_server", "claude_agent_sdk", "kimi_acp", "pi_rpc", "external_interactive"]},
                            "model": {"type": "string", "minLength": 1},
                            "effort": {"type": "string", "minLength": 1},
                            "service_tier": {"type": "string", "minLength": 1},
                            "provider_cwd_hint": {"type": "string", "minLength": 1},
                            "owned_paths": {"type": "array", "items": {"type": "string", "minLength": 1}},
                            "resume_native_session_id": {"type": "string", "minLength": 1}
                        },
                        "required": ["name", "role", "provider"]
                    }
                },
                "required": ["team_run_id", "member"]
            }
        },
        {
            "name": "team_run_rename_member",
            "description": "Rename one ProviderRuntimeProjection for future coordination and Dashboard display without replacing its provider-native session or historical id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "member_run_id": {"type": "string"},
                    "name": {"type": "string", "minLength": 1}
                },
                "required": ["team_run_id", "member_run_id", "name"]
            }
        },
        {
            "name": "team_run_deactivate_member",
            "description": "Deactivate an idle, queued, waiting, reviewing, or blocked ProviderRuntimeProjection while preserving its history. An active provider turn must be interrupted first.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "member_run_id": {"type": "string"},
                    "reason": {"type": "string", "minLength": 1}
                },
                "required": ["team_run_id", "member_run_id", "reason"]
            }
        },
        {
            "name": "team_run_start",
            "description": "Reserve and start a planning AgentTeamRun asynchronously, returning its running projection and exact Workspace-scoped UI URL immediately. Agent Team modes are Codex app-server (codex_app_server), Claude Agent SDK streaming (claude_agent_sdk), Kimi ACP (kimi_acp), and Pi RPC (pi_rpc); declared external_interactive members are user-driven and skipped by the supervisor. Retired one-shot modes such as codex_exec and claude_cli are rejected and never Team fallbacks. Provider cwd is the member worktree or selected Workspace project_root, never store_root. Provider transcripts and thinking remain in provider-native sessions.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "max_concurrency": {"type": "integer", "minimum": 1, "default": 4},
                    "idle_timeout_s": {"type": "integer", "minimum": 1, "default": 120}
                },
                "required": ["team_run_id"]
            }
        },
        {
            "name": "team_run_cancel",
            "description": "Cancel a planning, waiting, or reviewing TeamRun. Running cancellation is rejected until cooperative provider interruption exists.",
            "inputSchema": {
                "type": "object",
                "properties": {"team_run_id": {"type": "string"}},
                "required": ["team_run_id"]
            }
        },
        {
            "name": "team_run_list",
            "description": "List team runs in the store (latest projection, append order). One Execution Space store holds every tenant bound to it, so pass project_binding_id to see only one project's runs and status to drop finished ones. Mission is derived through AgentTeam; Legacy Wave rows never participate.",
            "inputSchema": {"type": "object", "properties": {
                "project_binding_id": {"type": "string", "description": "Return only runs bound to this project."},
                "status": {"type": "string", "description": "Return only runs in this status, for example running."}
            }}
        },
        {
            "name": "team_run_status",
            "description": "Show one team run: the run row, every member run with its latest MemberAction, a canonical Message-fabric summary, and the live dashboard URL. Historical team_messages.jsonl rows are excluded.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string", "description": "Run id returned by team_run_create / team_run_list."},
                },
                "required": ["team_run_id"]
            }
        },
        {
            "name": "team_run_board_summary",
            "description": "Decision-shaped Works board digest for one TeamRun (issue #305): a single `summary` string, always under 500 chars, with counts by status (open/in_progress/blocked/review/done/cancelled), assigned vs unassigned, the claim-ready count, and one `member: idle|working|awaiting-review` line per active member. Use this instead of team_run_work_list when the question is 'what should I do next', not 'show me every Work'.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string", "description": "Run id returned by team_run_create / team_run_list."}
                },
                "required": ["team_run_id"]
            }
        },
        {
            "name": "team_run_host_inbox",
            "description": "Read canonical Host mail across only those TeamRuns explicitly bound to one exact provider-native Host surface/thread. This is the safe Plugin/App integration path: it never leaks another Host task's inbox. By default returns actionable mail; all=true includes terminal canonical delivery history.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "host_surface": {"type": "string", "description": "Provider-native Host surface, for example codex-app."},
                    "host_thread_id": {"type": "string", "description": "Exact native Host task/session id stored on AgentTeamRun."},
                    "all": {"type": "boolean", "default": false}
                },
                "required": ["host_surface", "host_thread_id"]
            }
        },
        {
            "name": "team_run_inbox",
            "description": "Read the canonical Message/MessageDelivery projection addressed to one ProviderRuntimeProjection (or the reserved host recipient). By default returns actionable mail; all=true includes terminal delivery history. Historical team_messages.jsonl rows and provider-native transcript/tool history are excluded.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "member_run_id": {"type": "string", "description": "ProviderRuntimeProjection id, or `host` for the Lead inbox."},
                    "all": {"type": "boolean", "default": false}
                },
                "required": ["team_run_id", "member_run_id"]
            }
        },
        {
            "name": "team_inbox_list",
            "description": "Read one AgentTeam's shared Team Inbox (DOC-106): Team-subject canonical MessageDeliveries joined with their immutable Messages, including delivery status, claim binding, correlation, and author/source-Team provenance. Team-addressed peer Messages land here without waking every Member until one exact TeamMembership generation claims the delivery. Read-only; by default returns unclaimed queued items, all=true includes the full delivery history.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_id": {"type": "string", "description": "Exact AgentTeam id."},
                    "all": {"type": "boolean", "default": false}
                },
                "required": ["team_id"]
            }
        },
        {
            "name": "team_run_answer_message",
            "description": "Answer a provider-originated correlated question or plan-review Message as the authenticated AgentTeam Host. The exact response Message is durably published before the request delivery is ACKed, so an exact retry can recover a crash between those writes without duplicating the answer.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "message_id": {"type": "string"},
                    "option_id": {"type": "string", "description": "Exact option id exposed by the provider message."},
                    "response_text": {"type": "string", "description": "Free-form response only when the provider request exposes no exact options."}
                },
                "required": ["team_run_id", "message_id"]
            }
        },
        {
            "name": "team_run_steer_member",
            "description": "Inject operator or Lead input into a currently active provider turn. This is capability-gated and currently requires codex_app_server; when no turn is active, publish a canonical Message through the normal AgentTeam message surface.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "member_run_id": {"type": "string"},
                    "content": {"type": "string", "minLength": 1},
                    "requested_by": {"type": "string", "default": "host"}
                },
                "required": ["team_run_id", "member_run_id", "content"]
            }
        },
        {
            "name": "team_run_interrupt_member",
            "description": "Cooperatively interrupt one active provider turn when its execution mode advertises supports_cancel. Codex app-server uses turn/interrupt, Kimi ACP uses session/cancel, and Claude Agent SDK uses query.interrupt while preserving its native session.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "member_run_id": {"type": "string"},
                    "reason": {"type": "string"},
                    "requested_by": {"type": "string", "default": "host"}
                },
                "required": ["team_run_id", "member_run_id"]
            }
        },
        {
            "name": "team_run_close_member",
            "description": "Explicitly close one Member runtime while preserving the same ProviderRuntimeProjection, native-session binding, and frozen mailbox for a later reopen. Managed adapters release their Harness-owned process; external_interactive only closes Harness coordination because its process is user-owned. The live request must be sent through the same Host server process that started the TeamRun.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "member_run_id": {"type": "string"},
                    "reason": {"type": "string"},
                    "requested_by": {"type": "string", "default": "host"}
                },
                "required": ["team_run_id", "member_run_id"]
            }
        },
        {
            "name": "team_run_reopen_member",
            "description": "Reopen a closed ProviderRuntimeProjection in place. The exact Host may explicitly switch managed ↔ external_interactive only while closed; the transition advances one fenced generation and never imports an external transcript. Retired members cannot reopen.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "member_run_id": {"type": "string"},
                    "reason": {"type": "string"},
                    "reopened_by": {"type": "string", "default": "host"},
                    "host_runtime_mode": {"type": "string", "enum": ["managed", "external_interactive"]},
                    "execution_mode": {"type": "string", "description": "Required persistent provider mode when switching an external Host to managed."},
                    "host_thread_id": {"type": "string", "description": "Optional pull-session locator when switching to external_interactive."},
                    "max_concurrency": {"type": "integer", "minimum": 1, "default": 4},
                    "idle_timeout_s": {"type": "integer", "minimum": 1, "default": 120}
                },
                "required": ["team_run_id", "member_run_id"]
            }
        },
        {
            "name": "team_run_events",
            "description": "Read a team run's folded event log, ordered by seq. Pass `after_seq` (the last seq you already saw) to resume incrementally; events cover team_run/member_run/message/member_action lifecycle rows with host or member source kind.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "team_run_id": {"type": "string"},
                    "after_seq": {"type": "integer", "description": "Only return events with seq greater than this (default 0 = all)."}
                },
                "required": ["team_run_id"]
            }
        }
    ])
}
