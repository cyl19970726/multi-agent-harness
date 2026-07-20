# Company OS V2 actor identity system

## Purpose

Organization and collaboration require stable visual identity. Reusing one
generic robot icon for every Standing Agent makes reporting lines, activity,
ownership, and handoff difficult to scan. V2 assigns each durable Agent a
recognizable avatar while preserving actor-type semantics.

## Avatar family

Standing Agent avatars use one coherent family: sophisticated editorial 3D
portrait icons, head-and-shoulders framing, warm ivory circular ground,
graphite/sage/coral/amber accents, soft studio light, crisp silhouette, no text,
no logos, and no photorealistic human impersonation. They should feel capable
and professional rather than toy-like.

| Agent | Visual character | Accent | Recognition cue |
| --- | --- | --- | --- |
| Company Lead | composed coordinator, calm forward gaze | deep sage + warm gold | subtle orchestration halo/ring |
| Document Architecture Agent | thoughtful information architect | slate blue + ivory | layered page/structure motif |
| Finance Agent | precise, trusted controller | forest green + brass | restrained ledger/grid motif |
| Content Strategy Agent | expressive editorial strategist | coral + warm sand | flowing speech/page motif |
| Trademark Agent | vigilant legal/IP specialist | burnt orange + graphite | subtle shield/registration motif |

## Actor-type treatment

- Human: real profile photo or Human-specific illustrated portrait, always
  labelled `Human` when authority matters.
- Standing Agent: generated Agent avatar plus explicit `Standing Agent` or
  `Lead Agent` label; avatar alone never grants authority.
- External: neutral blue-gray portrait or organization mark plus `External`.
- temporary MemberRun: may use an execution-specific variant or badge but never
  silently reuse a Standing Agent identity without an explicit participation
  link.
- Proposed Agent: may preview its intended avatar with dashed coral boundary
  and `Proposed`; it must not appear Active.

The avatar is presentation metadata. ActorRef remains the canonical identity,
and permissions, availability, responsibility, and runtime are never inferred
from appearance.
