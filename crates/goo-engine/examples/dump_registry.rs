//! Print the assembled registry as pretty JSON — for parity-diffing against the
//! bash engine's registry.json:
//!
//!   COSMIC_GOO_BUILTIN_PLUGINS_DIR=$PWD/plugins \
//!     diff <(jq -S . registry.json) \
//!          <(cargo run -q --example dump_registry | jq -S .)
fn main() {
    let reg = goo_engine::registry::load_all();
    println!("{}", serde_json::to_string_pretty(&reg).unwrap());
}
