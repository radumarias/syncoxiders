//! Print the endpoint ID of a reported Oxfer share, for the relay's `access.denylist`.
//!
//! The command-line wrapper around [`p2p_transfer::node::share_endpoint_id`], whose
//! documentation gives the input it accepts (a share link, its fragment or the bare ticket)
//! and the form of the ID it returns. It decodes offline: it opens no connection, so the
//! sharer is never contacted, and it does not open the link.
//!
//! Run it from `p2p-transfer/`, so that directory's `rust-toolchain` applies:
//!
//! ```sh
//! cargo run -q -p p2p-transfer --example ticket-endpoint-id -- '<share link, fragment or ticket>'
//! ```
//!
//! Pass the input as the only argument, quoted: a link contains `&`, which the shell would
//! otherwise read as "run in the background". The argument `-` reads the input from standard
//! input instead (paste it, then press Ctrl-D).
//!
//! On success the ID is the only line on standard output. If the input cannot be read or
//! holds no ticket, a fixed message goes to standard error and the exit status is 1; with no
//! argument or more than one, a usage line and status 2. Where the ID goes on the relay, and
//! how the block is recorded: `deploy/relay/README.md` ("Abuse blocking") and
//! `docs/compliance/incident-runbook.md`.
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

use std::io::Read;
use std::process::ExitCode;

/// A share link is well under a kilobyte; this only bounds a mistaken paste.
const MAX_STDIN: u64 = 64 * 1024;

const USAGE: &str = "usage: cargo run -q -p p2p-transfer --example ticket-endpoint-id -- \
                     '<share link, fragment or ticket>' (or '-' to read standard input)";

fn main() -> ExitCode {
    // `args_os`, not `args`: `args` panics on an argument that is not valid UTF-8, and its
    // panic message repeats that argument, capability included.
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let [arg] = args.as_slice() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let Some(arg) = arg.to_str() else {
        eprintln!("the argument is not valid UTF-8 text; argument not shown");
        return ExitCode::FAILURE;
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
        arg.to_owned()
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
