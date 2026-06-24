fn main() {
    uniffi::generate_scaffolding("src/hambur_uniffi.udl")
        .expect("failed to generate UniFFI scaffolding");
}
