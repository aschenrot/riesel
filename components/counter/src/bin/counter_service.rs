fn main() -> std::io::Result<()> {
    riesel_protocol::run_component_from_env(riesel_counter_component::run_counter_server)
}
