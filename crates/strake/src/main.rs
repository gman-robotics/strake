use std::env;
use std::process;

use strake::{build, check, fmt, run, test, Exit, Opts, CHECK_USAGE};

fn main() {
    let exit = dispatch(env::args().skip(1).collect());
    if !exit.stdout.is_empty() {
        print!("{}", exit.stdout);
    }
    if !exit.stderr.is_empty() {
        eprint!("{}", exit.stderr);
    }
    process::exit(exit.code);
}

fn dispatch(args: Vec<String>) -> Exit {
    let mut args = args;
    if args.is_empty() {
        return usage_top();
    }
    match args.remove(0).as_str() {
        "build" => build(),
        "check" => dispatch_check(args),
        "fmt" => dispatch_fmt(args),
        "test" => dispatch_test(args),
        "run" => dispatch_run(args),
        _ => usage_top(),
    }
}

fn usage_top() -> Exit {
    Exit {
        code: 2,
        stdout: String::new(),
        stderr: "usage: strake check | fmt | test | run | build\n".into(),
    }
}

fn dispatch_check(mut args: Vec<String>) -> Exit {
    let mut opts = Opts::default();
    while let Some(flag) = args.first() {
        match flag.as_str() {
            "--draft" => {
                opts.draft = true;
                args.remove(0);
            }
            "--fn" => {
                args.remove(0);
                if args.is_empty() {
                    return usage_check();
                }
                opts.only_fn = Some(args.remove(0));
            }
            "--blob-root" => {
                args.remove(0);
                if args.is_empty() {
                    return usage_check();
                }
                opts.blob_root = Some(args.remove(0));
            }
            _ => break,
        }
    }
    if args.len() != 1 {
        return usage_check();
    }
    check(&args[0], opts)
}

fn usage_check() -> Exit {
    Exit {
        code: 2,
        stdout: String::new(),
        stderr: format!("{CHECK_USAGE}\n"),
    }
}

fn dispatch_fmt(args: Vec<String>) -> Exit {
    if args.len() != 1 {
        return Exit {
            code: 2,
            stdout: String::new(),
            stderr: "usage: strake fmt <file>\n".into(),
        };
    }
    fmt(&args[0])
}

fn dispatch_test(mut args: Vec<String>) -> Exit {
    let mut opts = Opts::default();
    if let Some(flag) = args.first() {
        if flag == "--draft" {
            opts.draft = true;
            args.remove(0);
        }
    }
    if args.len() != 1 {
        return Exit {
            code: 2,
            stdout: String::new(),
            stderr: "usage: strake test [--draft] <file>\n".into(),
        };
    }
    test(&args[0], opts)
}

fn dispatch_run(mut args: Vec<String>) -> Exit {
    let mut opts = Opts::default();
    let mut fn_name = None;
    while let Some(flag) = args.first() {
        match flag.as_str() {
            "--draft" => {
                opts.draft = true;
                args.remove(0);
            }
            "--fn" => {
                args.remove(0);
                if args.is_empty() {
                    return usage_run();
                }
                fn_name = Some(args.remove(0));
            }
            _ => break,
        }
    }
    let fn_name = match fn_name {
        Some(n) => n,
        None => return usage_run(),
    };
    if args.len() != 1 {
        return usage_run();
    }
    run(&args[0], &fn_name, opts)
}

fn usage_run() -> Exit {
    Exit {
        code: 2,
        stdout: String::new(),
        stderr: "usage: strake run --fn NAME [--draft] <file>\n".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::dispatch;
    use strake::CHECK_USAGE;

    #[test]
    fn dispatch_build() {
        let exit = dispatch(vec!["build".into()]);
        assert_eq!(exit.code, 2);
        assert_eq!(exit.stdout, "build is post-MVP\n");
        assert!(exit.stderr.is_empty());
    }

    #[test]
    fn check_usage_includes_blob_root() {
        assert!(CHECK_USAGE.contains("[--blob-root DIR]"));
        let exit = dispatch(vec!["check".into()]);
        assert!(exit.stderr.contains("[--blob-root DIR]"));
    }
}
