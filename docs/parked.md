# Parked ideas

Each entry is decided, not abandoned: it gives the reason it's paused and the condition for picking it back up.

## Garage desktop app with 3D vehicle models

**Idea (maintainer, 2026-11-14):** an egui desktop app showing the user's garage, with a 3D model of each vehicle chosen by its chassis code (E36, E46, E90, G05), alongside its history, faults and service log. Candidate stack: `egui` + `three-d` for rendering + `gltf` for loading.
**Parked because:** a GUI is out of v0 in DR-0001 and DR-0002, and it would be a third record while two are in flight.
**Resume when:** DR-0002 (garage) is accepted and its first slice has landed, so the app has real data to show.
**Known constraint:** 3D models carry their own licenses (Sketchfab models range from CC-BY to store purchases with no redistribution), and they depict BMW vehicles. Don't bundle any model whose license doesn't permit redistribution. The likely design is user-supplied glTF models from a local folder, with a generic fallback model we ship.
