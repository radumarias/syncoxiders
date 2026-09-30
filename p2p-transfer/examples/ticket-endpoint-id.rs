//! Print the endpoint ID of a reported Oxfer share, for the relay's `access.denylist`.
//!
//! An abuse report carries a share link, or the part of it the abuse page asks for: from
//! `endpoint` up to, but not including, `&cap=`. The relay blocks a share by its endpoint ID,
//! and that ID is encoded inside the link's ticket where it cannot be read by eye. This tool
//! decodes it offline. It opens no connection, so the sharer is never contacted, and it does
//! not open the link.
//!
//! Run it from `p2p-transfer/`, so that directory's `rust-toolchain` applies:
//!
//! ```sh
//! cargo run -q -p p2p-transfer --example ticket-endpoint-id -- '<share link or fragment>'
//! ```
//!
//! Input, as the only argument:
//!
//! - the whole link, `https://oxfer.app/#endpoint…&cap=…`;
//! - its fragment, with or without the `#`;
//! - the bare ticket, the part that starts with `endpoint` (what the abuse page asks for).
//!
//! Quote it: a link contains `&`, which the shell would otherwise read as "run in the
//! background". Whitespace and line breaks inside it, as in a link wrapped in an email, are
//! ignored, and so are `<`, `>` and quotes around it. The argument `-` reads the input from
//! standard input instead (paste it, then press Ctrl-D).
//!
//! Output: one line on standard output, the endpoint ID as 64 lowercase hex digits. That is
//! the form iroh-relay 1.1.0 parses in `access.denylist = ["<endpoint id>"]` (as
//! `Vec<EndpointId>`, each entry through `PublicKey::from_str`). Nothing else goes to
//! standard output. If no ticket can be read, a fixed message goes to standard error and the
//! exit status is 1; with no argument or more than one, a usage line and status 2. Where the
//! ID goes on the relay, and how the block is recorded: `deploy/relay/README.md` ("Abuse
//! blocking") and `docs/compliance/incident-runbook.md`.
//!
//! The capability after `cap=` is a bearer secret: anyone holding it can download the share.
//! The tool ignores it, never prints or logs it, and no message repeats the input. Two
//! things outside the tool can still show it, so prefer to pass only the ticket, which is
//! all the tool needs:
//!
//! - the shell keeps command-line arguments in its history (`-` and standard input avoid
//!   that);
//! - `cargo run` without `-q` echoes the command line, argument included, in its `Running`
//!   status line on standard error.
//!
//! The same decoding is `p2p_transfer::node::share_endpoint_id`, which the unit tests check
//! against iroh-relay's parse.

use std::io::Read;
use std::process::ExitCode;

/// A share link is well under a kilobyte; this only bounds a mistaken paste.
const MAX_STDIN: u64 = 64 * 1024;

const USAGE: &str = "usage: cargo run -q -p p2p-transfer --example ticket-endpoint-id -- \
                     '<share link, fragment or ticket>' (or '-' to read standard input)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [arg] = args.as_slice() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };

    let from_stdin = arg == "-";
    let input = if from_stdin {
        let mut input = String::new();
        if std::io::stdin()
            .take(MAX_STDIN)
            .read_to_string(&mut input)
            .is_err()
        {
            eprintln!("could not read standard input as UTF-8 text");
            return ExitCode::FAILURE;
        }
        input
    } else {
        arg.clone()
    };

    match p2p_transfer::node::share_endpoint_id(&input) {
        Ok(endpoint_id) => {
            println!("{endpoint_id}");
            if !from_stdin && input.contains("cap=") {
                eprintln!(
                    "note: the link's access code (cap=) is not needed and was ignored. To keep \
                     it out of shell history and cargo's output, pass only the part before \
                     &cap=, or pass '-' and paste the link on standard input."
                );
            }
            ExitCode::SUCCESS
        }
        Err(reason) => {
            eprintln!("{reason}");
            ExitCode::FAILURE
        }
    }
}
