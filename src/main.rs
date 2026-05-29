use clap::FromArgMatches;

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let command = mit::cli::build_command();
    let cli = match command.try_get_matches_from(mit::cli::normalize_args_for_clap(&args)) {
        Ok(matches) => match mit::cli::Cli::from_arg_matches(&matches) {
            Ok(cli) => cli,
            Err(error) => error.exit(),
        },
        Err(error) => {
            error.exit();
        }
    };

    if let Err(error) = mit::run(cli) {
        eprintln!("❌ {error}");
        std::process::exit(1);
    }
}
