use std::io;
use std::net::SocketAddr;

fn main() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let address = match args.next() {
        Some(address) => address,
        None => std::env::var("RIESEL_BIND_ADDRESS").map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "usage: counter_service <bind-address> (or set RIESEL_BIND_ADDRESS)",
            )
        })?,
    };
    if args.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: counter_service <bind-address>",
        ));
    }
    let address: SocketAddr = address
        .parse()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;

    riesel_counter_component::run_counter_server(&address.to_string())
}
