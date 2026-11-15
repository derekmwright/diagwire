# Parked ideas

Each entry is decided, not abandoned: it gives the reason it's paused and the condition for picking it back up.

## Community-contributed definitions (an open, licensed code database)

**Idea (maintainer, 2026-11-14):** contributors grow diagwire's job and fault-code definitions from their own cars, building the openly licensed BMW code database that doesn't exist today.
**Update 2026-11-15:** the fault-code part is no longer parked. It is [DR-0003](decisions/0003-code-reference.md). What stays parked here is contributed job and live-data definitions.
**Parked because:** it depends on the declarative definition format, which DR-0001 D5 defers until a verified BMW-specific job needs it.
**Resume when:** D5's format lands with its first verified job. The contribution pipeline gets its own design record.
**Known requirements:** every contribution carries a canonical-client capture with a D4 stamp; contributors' VINs, MACs and IPs are stripped by an automated CI check before merge; contributors certify (DCO sign-off) that it's decoded evidence from their own vehicle, not copied SGBD/ODX tables (D8); contributed text is untrusted input to any automation (security addendum A1).

## Battery overview: locate every battery's module and show its state

**Idea (maintainer, 2026-11-15):** on cars with more than one battery, find which control units manage each one and show status and health for all of them in one view.
**Parked because:** it needs BMW-specific live-data reads. DR-0001 D6 allows no data-identifier read until that identifier has its own reviewed allowlist entry, and D5 defers the definition format until the first verified BMW-specific job.
**Resume when:** DR-0001's read-only MCP slice has landed and D5 is reopened. This is a good candidate for the first verified BMW-specific read, so it can be the feature that reopens D5.
**Known requirements:** which batteries a given car has and which unit reports each is unverified per platform and must come from the car, not from memory; every value needs an authority to check against (an independent tool's reading, or a multimeter for voltage); values are shown with their source unit and evidence level; read-only. It pairs naturally with battery registration (DR-0004), which should check battery state first.

## Garage desktop app with 3D vehicle models

**Idea (maintainer, 2026-11-14):** an egui desktop app showing the user's garage, with a 3D model of each vehicle chosen by its chassis code (E36, E46, E90, G05), alongside its history, faults and service log. Candidate stack: `egui` + `three-d` for rendering + `gltf` for loading.
**Parked because:** a GUI is out of v0 in DR-0001 and DR-0002, and it would be a third record while two are in flight.
**Resume when:** DR-0002 (garage) is accepted and its first slice has landed, so the app has real data to show.
**Known constraint:** 3D models carry their own licenses (Sketchfab models range from CC-BY to store purchases with no redistribution), and they depict BMW vehicles. Don't bundle any model whose license doesn't permit redistribution. The likely design is user-supplied glTF models from a local folder, with a generic fallback model we ship.
