//! Whether a shell call could have changed a file.
//!
//! Every `Bash` / `PowerShell` call used to be an `opaque_edit` — the facts
//! said "21,805 calls could have changed files and left no diff" across the
//! pinned corpus, and that number *was* the shell-call count. Most of them are
//! a `grep` or a `sed -n`.
//!
//! Narrowing it runs against the rule the project lives by, so the design is
//! the conservative direction of it: **a writer misclassified as read-only
//! becomes a zero that should have been an unknown**, while a reader
//! misclassified as a writer costs only a little precision. So this is an
//! **allow-list**. A call is read-only when every simple command in it is a
//! known non-writer and nothing in it can redirect, substitute or interpret;
//! everything else — including anything the tokenizer cannot split — stays
//! opaque.
//!
//! Measured 2026-08-27 over the pinned 405-transcript corpus: 6,430 of 21,805
//! shell calls (29.5%) are provably read-only. `by_tool.<shell>.calls` stays
//! the total, so that share is visible as the difference rather than being
//! taken on faith.

/// Commands that cannot write a file.
///
/// Derived from what the corpus actually runs, not from imagination: every
/// entry here appears as the head of a simple command in a real call. A few
/// carry a guard in [`WRITE_FLAGS`] or [`WRITE_ARGS`], because they are
/// read-only right up until one flag turns them into a writer.
const READ_ONLY: &[&str] = &[
    // shell builtins and no-ops
    ":",
    "[",
    "[[",
    "cd",
    "echo",
    "export",
    "false",
    "local",
    "printf",
    "pwd",
    "set",
    "shift",
    "test",
    "true",
    "unset",
    // reading files
    "cat",
    "head",
    "less",
    "more",
    "nl",
    "sed",
    "strings",
    "tail",
    // searching
    "ack",
    "ag",
    "egrep",
    "fgrep",
    "find",
    "grep",
    "rg",
    // listing
    "dir",
    "ls",
    "tree",
    // shaping text
    "column",
    "comm",
    "cut",
    "expand",
    "fold",
    "join",
    "paste",
    "rev",
    "sort",
    "tr",
    "unexpand",
    "uniq",
    "wc",
    // structured text
    "jq",
    "yq",
    // comparing
    "cmp",
    "diff",
    // digests and file facts
    "b2sum",
    "basename",
    "cksum",
    "df",
    "dirname",
    "du",
    "file",
    "md5sum",
    "readlink",
    "realpath",
    "sha1sum",
    "sha256sum",
    "sha512sum",
    "stat",
    // asking the environment
    "date",
    "groups",
    "hostname",
    "hostnamectl",
    "id",
    "locale",
    "printenv",
    "tty",
    "type",
    "uname",
    "which",
    "whoami",
    // asking about processes and hardware
    "free",
    "htop",
    "lsblk",
    "lscpu",
    "lspci",
    "lsusb",
    "nproc",
    "nvidia-smi",
    "pgrep",
    "pidof",
    "ps",
    "top",
    "uptime",
    // logs
    "dmesg",
    "journalctl",
    // encoding, to stdout
    "base64",
    "hexdump",
    "od",
    "xxd",
    // waiting and counting
    "seq",
    "sleep",
    "yes",
    // PowerShell: the read verbs and the pipeline shapers. `Set-*`, `Out-File`
    // and `Remove-*` are deliberately absent.
    "ConvertFrom-Json",
    "ConvertTo-Json",
    "Format-List",
    "Format-Table",
    "Format-Wide",
    "Get-ChildItem",
    "Get-Command",
    "Get-Content",
    "Get-Date",
    "Get-Host",
    "Get-Item",
    "Get-Location",
    "Get-Member",
    "Get-Process",
    "Get-Service",
    "Get-Variable",
    "Group-Object",
    "Measure-Object",
    "Out-Host",
    "Out-String",
    "Select-Object",
    "Select-String",
    "Sort-Object",
    "Where-Object",
    "Write-Host",
    "Write-Output",
];

/// **Deliberately absent: every command-prefix wrapper.** `env`, `command`,
/// `sudo`, `timeout`, `nice`, `nohup`, `time`, `stdbuf`, `setsid` and `xargs`
/// all take another command as their operand and run it, so allow-listing the
/// wrapper allow-lists everything — `env FOO=1 rm -rf x` reads as `env`.
/// `env` was on this list until it was audited against a second implementation
/// of the same rule. That was not theoretical: **eight** corpus calls run
/// `env -i … copilot`, `env -u … just publish` and `env HOME=… cargo` and were
/// being called read-only. Two more — a bare `env | grep` — are the price of
/// leaving the whole family off, and it is the right way round.
///
/// Heads whose verdict depends on the **subcommand**, with the subcommands
/// that cannot write.
///
/// `git` is the whole table because it is the only multiplexer in the corpus
/// worth the precision: `git status` and `git commit` are the same head. The
/// write subcommands — `add`, `commit`, `checkout`, `push`, `stash`, `config`,
/// `remote`, `tag` — are simply absent, which is what leaves them opaque.
/// `cargo`, `just`, `gh`, `npm`, `docker` and `systemctl` are deliberately not
/// here: they are not worth a second table each, and everything they run stays
/// an unknown.
const SUBCOMMANDS: &[(&str, &[&str])] = &[(
    "git",
    &[
        "blame",
        "branch",
        "cat-file",
        "check-ignore",
        "count-objects",
        "describe",
        "diff",
        "diff-index",
        "diff-tree",
        "for-each-ref",
        "grep",
        "log",
        "ls-files",
        "ls-remote",
        "ls-tree",
        "merge-base",
        "name-rev",
        "rev-list",
        "rev-parse",
        "shortlog",
        "show",
        "show-ref",
        "status",
        "var",
        "whatchanged",
    ],
)];

/// Flags that turn an otherwise read-only command into a writer, keyed by the
/// command — or by `"<head> <subcommand>"` where the head is a multiplexer.
///
/// Matched as the whole word or as a prefix, so `sed -i.bak` is caught along
/// with `sed -i`. Note this is *not* applied to `grep`, where `-i` means
/// case-insensitive: the guard is per command, exactly so that it can be.
const WRITE_FLAGS: &[(&str, &[&str])] = &[
    ("sed", &["-i", "--in-place"]),
    (
        "git branch",
        &[
            "-d",
            "-D",
            "-m",
            "-M",
            "-c",
            "-C",
            "-f",
            "-u",
            "--delete",
            "--move",
            "--copy",
            "--force",
            "--set-upstream-to",
            "--unset-upstream",
            "--edit-description",
        ],
    ),
];

/// Arguments that turn an otherwise read-only command into a writer, matched
/// as the whole word.
///
/// `find` is the case this exists for: it reads directories until `-delete` or
/// `-exec` hands it a command kagviz is not going to parse.
const WRITE_ARGS: &[(&str, &[&str])] = &[(
    "find",
    &[
        "-delete", "-exec", "-execdir", "-ok", "-okdir", "-fls", "-fprint", "-fprint0", "-fprintf",
    ],
)];

/// `git`'s own options that take a separate argument, so `git -C /some/path
/// status` does not read `/some/path` as the subcommand.
const OPT_WITH_ARG: &[&str] = &[
    "-C",
    "-c",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--exec-path",
];

/// Redirect targets that are not a file. `> /dev/null` and `2>&1` are the
/// overwhelming majority of `>` in the corpus and neither writes anything.
const NOT_A_FILE: &[&str] = &["/dev/null", "/dev/stdout", "/dev/stderr", "/dev/tty"];

/// One token of a command line.
#[derive(Debug, PartialEq)]
enum Tok {
    Word(String),
    /// A separator between simple commands: `;`, `&&`, `||`, `|`, `&`, newline.
    Sep,
    /// A redirection operator. `true` when it writes (`>`, `>>`, `&>`).
    Redirect(bool),
}

/// Whether a shell call provably changed no file.
///
/// `false` is the safe answer and the default: it means "kagviz cannot rule
/// out a write", which is what `opaque_edits` counts.
pub fn wrote_nothing(command: &str) -> bool {
    let Some(tokens) = tokenize(command) else {
        return false;
    };
    let Some(words) = strip_redirects(tokens) else {
        return false;
    };
    // A standalone `{` or `}` opens a brace group (bash) or a script block
    // (PowerShell), and its body is commands this function is not reading:
    // `Where-Object { Remove-Item $_ }` would otherwise pass on its head
    // alone. Brace *expansion* is untouched — `ls {a,b}.txt` is one word.
    if words
        .iter()
        .any(|t| matches!(t, Tok::Word(w) if w == "{" || w == "}"))
    {
        return false;
    }
    words.split(|t| *t == Tok::Sep).all(read_only)
}

/// Split a command line into words and separators, or refuse.
///
/// Refusing is the whole point of the return type. Command substitution,
/// a heredoc, a subshell and an unterminated quote all mean the call could be
/// running something this function is not reading, and the honest answer to
/// that is the unknown rather than a guess.
fn tokenize(s: &str) -> Option<Vec<Tok>> {
    let b: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    // `None` is "between words"; `Some("")` is a word that has been opened and
    // is still empty — `grep "" f` passes an empty argument, and losing it
    // would silently merge two words into one.
    let mut cur: Option<String> = None;
    let mut i = 0;

    macro_rules! flush {
        () => {
            if let Some(word) = cur.take() {
                out.push(Tok::Word(word));
            }
        };
    }
    macro_rules! open {
        () => {
            cur.get_or_insert_with(String::new)
        };
    }

    while i < b.len() {
        match b[i] {
            '\\' => {
                // A backslash-newline is a line continuation and joins nothing.
                let next = *b.get(i + 1)?;
                if next != '\n' {
                    open!().push(next);
                }
                i += 2;
            }
            '\'' => {
                let end = b[i + 1..].iter().position(|c| *c == '\'')? + i + 1;
                open!().extend(&b[i + 1..end]);
                i = end + 1;
            }
            '"' => {
                let word = open!();
                let mut j = i + 1;
                loop {
                    match *b.get(j)? {
                        '"' => break,
                        '\\' => {
                            word.push(*b.get(j + 1)?);
                            j += 2;
                        }
                        // Command substitution inside double quotes still runs.
                        '`' => return None,
                        '$' if b.get(j + 1) == Some(&'(') => return None,
                        c => {
                            word.push(c);
                            j += 1;
                        }
                    }
                }
                i = j + 1;
            }
            '`' => return None,
            '$' if b.get(i + 1) == Some(&'(') => return None,
            // A subshell reorders everything this function assumes.
            '(' | ')' => return None,
            '<' => {
                // A heredoc's body is arbitrary text that would be tokenized as
                // if it were shell. It is also where most shell file-writing in
                // this corpus happens.
                if b.get(i + 1) == Some(&'<') {
                    return None;
                }
                flush!();
                out.push(Tok::Redirect(false));
                i = eat_fd_dup(&b, i + 1, &mut out);
            }
            '>' => {
                flush!();
                out.push(Tok::Redirect(true));
                i += if b.get(i + 1) == Some(&'>') { 2 } else { 1 };
                i = eat_fd_dup(&b, i, &mut out);
            }
            '&' => {
                flush!();
                match b.get(i + 1) {
                    // `&>` and `&>>` redirect both streams.
                    Some('>') => {
                        out.push(Tok::Redirect(true));
                        i += if b.get(i + 2) == Some(&'>') { 3 } else { 2 };
                        i = eat_fd_dup(&b, i, &mut out);
                    }
                    Some('&') => {
                        out.push(Tok::Sep);
                        i += 2;
                    }
                    // A bare `&` backgrounds what came before it.
                    _ => {
                        out.push(Tok::Sep);
                        i += 1;
                    }
                }
            }
            '|' => {
                flush!();
                out.push(Tok::Sep);
                // `||` and `|&` are both separators, two characters wide.
                i += if matches!(b.get(i + 1), Some('|') | Some('&')) {
                    2
                } else {
                    1
                };
            }
            ';' | '\n' => {
                flush!();
                out.push(Tok::Sep);
                i += 1;
            }
            ' ' | '\t' | '\r' => {
                flush!();
                i += 1;
            }
            c => {
                open!().push(c);
                i += 1;
            }
        }
    }
    flush!();
    Some(out)
}

/// Consume a file-descriptor duplication (`&1` in `2>&1`, `&-` in `>&-`) as
/// the redirect's target, so it is not mistaken for a background separator.
fn eat_fd_dup(b: &[char], i: usize, out: &mut Vec<Tok>) -> usize {
    if b.get(i) != Some(&'&') {
        return i;
    }
    let mut j = i + 1;
    while b.get(j).is_some_and(char::is_ascii_digit) {
        j += 1;
    }
    if j == i + 1 && b.get(j) == Some(&'-') {
        j += 1;
    }
    if j > i + 1 {
        out.push(Tok::Word(b[i..j].iter().collect()));
        return j;
    }
    i
}

/// Drop the redirections, refusing on any that writes to a real file.
///
/// A bare file-descriptor number in front of a redirect (`2` in `2> log`)
/// belongs to the redirect, not to the command, and goes with it.
fn strip_redirects(tokens: Vec<Tok>) -> Option<Vec<Tok>> {
    let mut out: Vec<Tok> = Vec::with_capacity(tokens.len());
    let mut it = tokens.into_iter();
    while let Some(tok) = it.next() {
        let Tok::Redirect(writes) = tok else {
            out.push(tok);
            continue;
        };
        let Some(Tok::Word(target)) = it.next() else {
            return None;
        };
        if writes && !(target.starts_with('&') || NOT_A_FILE.contains(&target.as_str())) {
            return None;
        }
        if matches!(out.last(), Some(Tok::Word(w)) if w.chars().all(|c| c.is_ascii_digit())) {
            out.pop();
        }
    }
    Some(out)
}

/// Whether one simple command is a known non-writer.
fn read_only(cmd: &[Tok]) -> bool {
    let words: Vec<&str> = cmd
        .iter()
        .filter_map(|t| match t {
            Tok::Word(w) => Some(w.as_str()),
            _ => None,
        })
        .collect();
    // Leading `FOO=bar` assignments precede the command and write nothing.
    let start = words.iter().take_while(|w| is_assignment(w)).count();
    let Some(head) = words.get(start) else {
        // Nothing but assignments — or nothing at all, from a trailing `;`.
        return true;
    };
    let head = head.rsplit('/').next().unwrap_or(head);
    let args = &words[start + 1..];

    let sub = SUBCOMMANDS.iter().find(|(name, _)| *name == head);
    if sub.is_none() && !READ_ONLY.contains(&head) {
        return false;
    }
    if !passes_guards(head, args) {
        return false;
    }
    let Some((_, allowed)) = sub else {
        return true;
    };
    // Skip the multiplexer's own options to reach the subcommand.
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        if OPT_WITH_ARG.contains(arg) {
            i += 2;
        } else if arg.starts_with('-') {
            i += 1;
        } else {
            break;
        }
    }
    let Some(subcommand) = args.get(i) else {
        return false;
    };
    allowed.contains(subcommand) && passes_guards(&format!("{head} {subcommand}"), args)
}

/// Whether a command's arguments are free of the flags that would turn it
/// into a writer.
fn passes_guards(key: &str, args: &[&str]) -> bool {
    if let Some((_, flags)) = WRITE_FLAGS.iter().find(|(name, _)| *name == key)
        && args.iter().any(|a| flags.iter().any(|f| a.starts_with(f)))
    {
        return false;
    }
    if let Some((_, bad)) = WRITE_ARGS.iter().find(|(name, _)| *name == key)
        && args.iter().any(|a| bad.contains(a))
    {
        return false;
    }
    true
}

/// `NAME=value`, the shell's per-command environment prefix.
fn is_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_readers_are_read_only() {
        for cmd in [
            "grep -rn TODO src/",
            "cat README.md",
            "ls -la",
            "sed -n '1,50p' src/main.rs",
            "git status --short",
            "git log --oneline -8",
            "wc -l src/*.rs",
            "jq -r '.tokens.output' facts.json",
            "cd /home/ken/src && ls",
            "MSYS_NO_PATHCONV=1 grep -c x file",
            "/usr/bin/grep -n x file",
            "find src -name '*.sh'",
            "git branch --show-current",
            "Get-ChildItem | Select-Object Name",
        ] {
            assert!(wrote_nothing(cmd), "should be read-only: {cmd}");
        }
    }

    #[test]
    fn anything_that_could_write_stays_opaque() {
        for cmd in [
            "echo hi > out.txt",
            "cat a.txt >> b.txt",
            "sed -i 's/a/b/' f.rs",
            "sed -i.bak 's/a/b/' f.rs",
            "git commit -m wip",
            "git add -A",
            "git branch -D old-branch",
            "cargo build --release",
            "rm -rf target",
            "mkdir -p out",
            "find . -name '*.tmp' -delete",
            "find . -name '*.sh' -exec ls -l {} +",
            "ssh kubs0 'systemctl restart x'",
            "python3 -c 'open(\"f\",\"w\").write(\"x\")'",
            "grep x f | tee out.txt",
            "just check",
            "curl -o out.json https://example.invalid/",
            // read-only right up to the last command in the chain
            "grep -n x f && cp f g",
        ] {
            assert!(!wrote_nothing(cmd), "should be opaque: {cmd}");
        }
    }

    /// The three shapes that make a command unreadable rather than a writer.
    /// The verdict is the same — opaque — and the reason is that kagviz is not
    /// going to guess at what it did not parse.
    #[test]
    fn what_cannot_be_parsed_is_an_unknown_not_a_zero() {
        for cmd in [
            "echo $(date)",             // command substitution
            "echo `date`",              // the older spelling
            "(cd /tmp && ls)",          // subshell
            "cat <<'PY'\nprint(1)\nPY", // heredoc
            "grep 'unterminated",       // a quote that never closes
        ] {
            assert!(!wrote_nothing(cmd), "should be opaque: {cmd}");
        }
    }

    /// `2>&1` and `> /dev/null` are the overwhelming majority of `>` in the
    /// corpus, and neither writes a file. Reading them as writes left 6,118
    /// read-only calls opaque in an earlier draft of this.
    #[test]
    fn discarding_output_is_not_writing_a_file() {
        for cmd in [
            "grep -rn x src/ 2>/dev/null",
            "ls -l 2>&1 | head -20",
            "cat f 2>&1",
            "git status >/dev/null 2>&1",
            "jq . f.json 2>&1 | tail -5",
            "ls &> /dev/null",
        ] {
            assert!(wrote_nothing(cmd), "should be read-only: {cmd}");
        }
    }

    /// A wrapper that runs its operand allow-lists everything behind it. This
    /// is the class the whole design is most exposed to, so the answer is to
    /// keep every one of them off the list rather than to guard each.
    #[test]
    fn a_command_prefix_wrapper_is_never_read_only() {
        for cmd in [
            "env FOO=1 rm -rf out",
            "env",
            "command -v python3",
            "command rm -rf out",
            "sudo ls",
            "timeout 5 ls",
            "xargs rm < list",
            "nohup ./deploy.sh",
        ] {
            assert!(!wrote_nothing(cmd), "should be opaque: {cmd}");
        }
    }

    /// A script block's body never reaches [`read_only`] — it becomes
    /// arguments of the cmdlet in front of it — so the block itself has to be
    /// the refusal.
    #[test]
    fn a_brace_group_or_script_block_is_not_read_by_its_head() {
        assert!(!wrote_nothing(
            "Get-Process | Where-Object { $_.Name -like 'x' }"
        ));
        assert!(!wrote_nothing(
            "Get-ChildItem | Where-Object { Remove-Item $_ }"
        ));
        assert!(!wrote_nothing("{ ls; rm -rf out; }"));
        // Brace expansion is a word, not a group.
        assert!(wrote_nothing("ls src/{a,b}.rs"));
        assert!(wrote_nothing("grep -n 'import { x } from \"y\"' f.ts"));
    }

    /// The guard is per command precisely so that `-i` can mean two things.
    #[test]
    fn a_write_flag_on_one_command_is_a_read_flag_on_another() {
        assert!(wrote_nothing("grep -i pattern file"));
        assert!(!wrote_nothing("sed -i 's/a/b/' file"));
    }

    /// The whole chain has to be read-only, not just its first command — the
    /// direction the conservative rule points.
    #[test]
    fn one_writer_anywhere_makes_the_whole_call_opaque() {
        assert!(wrote_nothing("cd src && grep -n x f | wc -l"));
        assert!(!wrote_nothing(
            "cd src && grep -n x f | wc -l && touch done"
        ));
        assert!(!wrote_nothing("ls\ngrep x f\nrm f"));
    }

    /// A `git` subcommand that writes is left out of the table rather than
    /// blocked by a guard, and `-C <path>` must not be read as one.
    #[test]
    fn git_is_read_by_its_subcommand_not_by_its_name() {
        assert!(wrote_nothing("git -C /home/ken/src/korg status"));
        assert!(!wrote_nothing("git -C /home/ken/src/korg commit -m x"));
        assert!(
            !wrote_nothing("git"),
            "a bare multiplexer names no subcommand"
        );
    }
}
