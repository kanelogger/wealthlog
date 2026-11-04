fn main() {
    // embed_migrations! cannot tell Cargo that a newly added directory changes
    // its expansion. Rebuild the embedded set when any migration changes.
    println!("cargo:rerun-if-changed=migrations");
}
