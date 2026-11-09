macro_rules! eprintln {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), $($arg)*);
    }};
}

#[cfg(not(feature = "host"))]
mod device_app;
#[cfg(feature = "host")]
mod host_app;

fn main() {
    let mut args = std::env::args().skip(1);
    let first = args.next();
    if first.as_deref() == Some("--install-bootlogo") {
        let Some(card) = std::env::var_os("SLOT_ROOT") else {
            eprintln!("slot: --install-bootlogo needs SLOT_ROOT");
            std::process::exit(2);
        };
        let card = std::path::Path::new(&card);
        slot::bootlogo::refresh(card);
        slot::bootlogo::remember(card);
        return;
    }
    if first.as_deref() == Some("--init-root") {
        let Some(dir) = args.next() else {
            eprintln!("slot: --init-root needs a directory");
            std::process::exit(2);
        };
        slot::root::ensure(std::path::Path::new(&dir));
        return;
    }
    #[cfg(unix)]
    if first.as_deref() == Some("--pico8") {
        let (Some(card), Some(run)) = (
            std::env::var_os("SLOT_ROOT"),
            std::env::var_os(slot::loading_mark::RUN_VAR),
        ) else {
            eprintln!("slot: --pico8 needs SLOT_ROOT and SLOT_RUN");
            std::process::exit(2);
        };
        std::process::exit(slot::pico8::run(
            std::path::Path::new(&card),
            std::path::Path::new(&run),
        ));
    }
    slot::crash::install();

    #[cfg(feature = "host")]
    host_app::run();

    #[cfg(not(feature = "host"))]
    device_app::run();
}
