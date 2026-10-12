# Agensi Connection

A local installation connecting the owner's agent clients to the Agensi marketplace using an
explicitly selected outbound route.

## Language

**Route Mode**:
The owner's selected connection policy: disconnected, direct, or through one named proxy.
_Avoid_: Automatic fallback, automatic rotation

**Proxy Profile**:
A saved named proxy connection that can be selected without re-entering its address.
_Avoid_: Proxy chain

**Client Registration**:
An agent client's named reference to this local Agensi installation.
_Avoid_: Separate installation

**Skill Package**:
A skill's instruction files and supporting resources, authored locally or acquired and saved after an authorized installation.
_Avoid_: Listing

**Internal Operating Skill**:
Original reusable guidance for the owner's use of a service, not a marketplace product.
_Avoid_: Creator Listing

**Demand Snapshot**:
Dated public uptake observations such as installs and review counts, not evidence of paid transactions or revenue.
_Avoid_: Sales report

**Client Plugin**:
A client's supported distribution container for skills and related components, distinct from a skill being loaded or a service being authenticated.
_Avoid_: MCP Connection

**Creator Listing**:
A creator's marketplace submission for review and publication, distinct from installing a skill.
_Avoid_: Skill Package

**Owned Temporary Artifact**: Disposable working data belonging to one operation, unlike shared temporary storage.
**Runtime State**: Saved identity and user configuration; not disposable cache.
**Cleanup Preview**: An inventory of eligible removals without performing them.
**Resolver Observation**: A dated recursive DNS identity observed by a diagnostic service, not a location guarantee.

**Registration Transaction**: Coordinated changes to the intended client registrations, with verification and conditional restoration if completion fails.
**Recovery Evidence**: Private original configuration retained when restoring it would overwrite a concurrent change.
**Callback Authority**: The local destination identified by a browser's sign-in callback request.
