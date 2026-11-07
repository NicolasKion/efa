//! Every integration test in one binary: each test file used to link its own copy of the app,
//! which made a full test run link over a dozen huge binaries at once.

mod boosters;
mod browsers;
mod cargo;
mod characters;
mod columns;
mod drones;
mod eft;
mod implants;
mod item_stats;
mod mutations;
mod port;
mod prices;
mod profiles;
mod stats;
mod surroundings;
mod validation;
mod wizard;
