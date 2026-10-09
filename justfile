export NO_COLOR := "true"

# List available commands.
default:
    @just --list

# Build the native desktop interface.
build-desktop:
    cargo build --release --locked --no-default-features --features gui --bin tally-gui

# Run the native desktop interface.
run-desktop:
    cargo run --release --locked --no-default-features --features gui --bin tally-gui

# Build the command-line app.
build-cli:
    cargo build --release --locked --no-default-features --features cli --bin tally-problem

# Build the browser interface, optionally setting its hosting path.
build-web public_url="/":
    trunk build --release --public-url {{ quote(public_url) }}
