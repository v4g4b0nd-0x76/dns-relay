use clap::Parser;

fn main() -> std::process::ExitCode {
    match dns_relay_admin::run(dns_relay_admin::Cli::parse()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
