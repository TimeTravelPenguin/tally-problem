export NO_COLOR := "true"

# List available commands.
default:
    @just --list

# Build the native desktop interface.
build-desktop profile="release":
    cargo build --profile {{ quote(profile) }} --locked --no-default-features --features gui --bin tally-gui

# Run the native desktop interface.
run-desktop profile="release":
    cargo run --profile {{ quote(profile) }} --locked --no-default-features --features gui --bin tally-gui

# Build the command-line app.
build-cli profile="release":
    cargo build --profile {{ quote(profile) }} --locked --no-default-features --features cli --bin tally-problem

# Build the browser interface, optionally setting its hosting path.
build-web public_url="/" profile="web-release":
    trunk build --release={{ if profile == "dev" { "false" } else { "true" } }} --cargo-profile {{ quote(profile) }} --public-url {{ quote(public_url) }}
