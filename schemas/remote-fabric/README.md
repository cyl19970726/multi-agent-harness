# Remote Node Fabric schemas

`agentfirm.remote_fabric.v1` is the only accepted schema family for the Wave 5
transport foundation. These schemas describe transport trust and delivery
objects only. They deliberately do not authorize or encode Team Work,
Message, provider-session, or runtime business mutations.

Every object is closed with `additionalProperties: false`. Protocol negotiation
uses major version `1`; schema changes remain explicit through the schema
bundle digest exchanged during enrollment and `NodeHello`.

Fixtures live in `fixtures/valid` and `fixtures/invalid`. The executable gate is
`node scripts/acceptance-remote-fabric.mjs`.
