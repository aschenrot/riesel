fn main() -> std::io::Result<()> {
    riesel_protocol::run_component_from_env(riesel_storage_component::run_storage_server)
}
