use std::io;
use std::net::SocketAddr;

fn main() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let address = args.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: storage_service <bind-address>",
        )
    })?;
    if args.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: storage_service <bind-address>",
        ));
    }
    let address: SocketAddr = address
        .parse()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;

    riesel_storage_component::run_storage_server(&address.to_string())
}
