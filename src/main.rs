extern crate clap;
#[macro_use]
extern crate nom;

use nom::{IResult, not_line_ending, digit, hex_digit};
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::collections::BTreeMap;
use std::str::{self, FromStr};
use clap::App;

struct Parser {
}

#[derive(Debug, PartialEq, Eq, Hash)]
struct CPOW {
    line_no: u32,
    shim: bool,
}

// A CPOW only indirectly used by a test.
#[derive(Debug)]
struct IndirectCPOW {
    line_no: u32,
    shim: bool,
    filename: String,
}

enum SomeCPOW {
    CPOW(CPOW),
    Indirect(IndirectCPOW),
}

#[derive(Debug)]
struct Test {
    testname: String,
    cpows: Vec<CPOW>,
    indirect_cpows: Vec<IndirectCPOW>,
}

#[derive(Debug)]
enum LogLine {
    TestStart(String),
    StackComponent(u32, String, String, u32),
}

impl Parser {
    fn new() -> Parser {
        Parser { }
    }

    fn parse_file(self: &Parser, fname: &str) -> Vec<LogLine> {
        named!(test_start<&str>,
               ws!(do_parse!(take_until_and_consume_s!("TEST-START") >>
                             tag!("|") >>
                             path: not_line_ending >>
                             ( str::from_utf8(path).unwrap() ))));

        named!(parsed_number<u32>,
               map_res!(map_res!(ws!(digit), str::from_utf8), FromStr::from_str));

        named!(stack_component<(u32, &str)>,
               ws!(do_parse!(take_until_and_consume_s!("#") >>
                  component: parsed_number                  >>
                             opt!(tag!("0x"))               >>
                             hex_digit                      >>
                             alt!(tag!("i") | tag!("b"))    >>
                        loc: take_until_s!(" (")            >>
                             delimited!(tag!("("),
                                        is_not!(")"),
                                        tag!(")"))          >>

                             (component, str::from_utf8(loc).unwrap()))));

        // When finding the filename component of a path, returns the position
        // after the last slash or 0 if there wasn't a path component.
        fn last_slash_idx(s: &str) -> usize {
            if let Some(idx) = s.rfind('/') {
                idx + 1
            } else {
                0
            }
        }


        let mut parsed = Vec::new();
        let reader : Box<io::Read> = if fname != "-" {
            let file = File::open(fname);
            if file.is_err() {
                println!("Warning ({}): {}", fname, file.unwrap_err());
                return parsed;
            }
            Box::new(file.unwrap())
        } else {
            Box::new(io::stdin())
        };
        let f = BufReader::new(reader);
        for line in f.lines().filter_map(|r| r.ok()) {
            let bytes = line.into_bytes();
            if let IResult::Done(rest, path) = test_start(&bytes[..]) {
                assert!(rest.len() == 0);

                let last_slash = last_slash_idx(&path);
                parsed.push(LogLine::TestStart(String::from(&path[last_slash..])));
            } else if let IResult::Done(rest, (idx, loc)) = stack_component(&bytes[..]) {
                assert!(rest.len() == 0);

                let lineno_sep = loc.rfind(':').unwrap();
                let path = String::from(&loc[0..lineno_sep]);
                let fname = String::from(&path[last_slash_idx(&path)..]);
                let line_no = loc[lineno_sep + 1..].parse::<u32>().unwrap();

                parsed.push(LogLine::StackComponent(idx, path, fname, line_no));
            }
        }

        parsed
    }
}

struct CPOWFinder<'a> {
    idx: usize,
    lines: &'a [LogLine],
    peeked: bool,
    include_shims: bool,
}

impl<'a> CPOWFinder<'a> {
    // Returns the next line and leaves it.
    fn peek_line(&mut self) -> Option<&'a LogLine> {
        return if self.idx < self.lines.len() {
            self.peeked = true;
            Some(&self.lines[self.idx])
        } else {
            None
        };
    }

    // "takes" the previously peeked line.
    fn take_line(&mut self) {
        assert!(self.peeked, "take without corresponding peek");
        self.peeked = false;
        self.idx += 1;
    }

    // Combination peek + take if we know there's at least one line.
    fn next_line(&mut self) -> &'a LogLine {
        if let Some(ref line) = self.peek_line() {
            self.take_line();
            return line;
        }

        panic!("shouldn't be out of lines");
    }

    // Given a CPOW usage, returns information about the CPOW if it is from
    // the test we care about.
    fn parse_cpow(&mut self, testname: &str) -> Option<SomeCPOW> {
        fn is_test_path(p: &str) -> bool {
            p.starts_with("chrome://mochitests/") ||
                p.starts_with("chrome://mochikit/")
        }

        let mut report = false; // only report CPOWs from this test.
        let mut cpow = CPOW { line_no: 0, shim: false };
        let mut indirect_cpow = IndirectCPOW { line_no: 0, shim: false, filename: String::new() };
        match self.next_line() {
            &LogLine::StackComponent(idx, ref path, ref filename, line_no) => {
                assert!(idx == 0);
                if is_test_path(path) {
                    report = true;
                }

                if filename == testname {
                    // Direct CPOW, fill it in now.
                    cpow.line_no = line_no;
                } else if report {
                    // Indirect CPOW, start filling it in now.
                    indirect_cpow.line_no = line_no;
                    indirect_cpow.filename.push_str(path);
                }

                // Otherwise, we don't know what to do with this filename.
            }
            &LogLine::TestStart(_) => {
                panic!("bad line");
            }
        };

        while let Some(next_line) = self.peek_line() {
            match next_line {
                // Pull lines until we find the next test or the next CPOW.
                &LogLine::StackComponent(0, _, _, _) | &LogLine::TestStart(_) => break,
                &LogLine::StackComponent(_, ref path, ref filename, line_no) => {
                    self.take_line();
                    if (!report || cpow.line_no == 0) && is_test_path(path) {
                        report = true;

                        if cpow.line_no == 0 {
                            if testname == filename {
                                cpow.line_no = line_no;
                            } else if indirect_cpow.line_no == 0 {
                                // This is the first stack component in a
                                // test, use it.
                                indirect_cpow.line_no = line_no;
                                indirect_cpow.filename.push_str(path);
                            }
                        }
                    }
                    if !cpow.shim && filename == "RemoteAddonsParent.jsm" {
                        cpow.shim = true;
                        indirect_cpow.shim = true;
                    }
                }
            }
        }

        if report {
            if cpow.line_no != 0 {
                Some(SomeCPOW::CPOW(cpow))
            } else {
                Some(SomeCPOW::Indirect(indirect_cpow))
            }
        } else {
            None
        }
    }

    // Given a TEST-START, looks for and accumulates CPOW uses.
    fn parse_test(&mut self, testname: &str) -> Option<Test> {
        let mut cpows = Vec::new();
        let mut indirect_cpows = Vec::new();
        while let Some(next_line) = self.peek_line() {
            match next_line {
                &LogLine::TestStart(_) => break,
                &LogLine::StackComponent(_, _, _, _) => {
                    match self.parse_cpow(testname) {
                        Some(SomeCPOW::CPOW(c)) => {
                            if !c.shim || self.include_shims {
                                cpows.push(c)
                            }
                        }
                        Some(SomeCPOW::Indirect(i)) => {
                            if !i.shim || self.include_shims {
                                indirect_cpows.push(i)
                            }
                        }
                        None => {
                        }
                    }
                }
            }
        }

        if !cpows.is_empty() || !indirect_cpows.is_empty() {
            cpows.sort_by_key(|k| k.line_no);
            indirect_cpows.sort_by_key(|k| k.line_no);
            Some(Test { testname: String::from(testname), cpows: cpows, indirect_cpows: indirect_cpows })
        } else {
            None
        }
    }

    // Returns a list of tests that have CPOW uses.
    fn compile_cpows(lines: &[LogLine], include_shims: bool) -> Vec<Test> {
        let mut finder = CPOWFinder { idx: 0, lines: lines, peeked: false, include_shims: include_shims };
        let mut tests = Vec::new();
        while let Some(next_line) = finder.peek_line() {
            finder.take_line();
            match next_line {
                &LogLine::StackComponent(..) => {
                    panic!("unconsumed StackComponent?");
                }
                &LogLine::TestStart(ref fname) => {
                    if let Some(test) = finder.parse_test(fname) {
                        tests.push(test);
                    }
                }
            }
        }

        tests
    }
}

fn main() {
    let matches = App::new("cpow-log-parser")
                          .version("1.0")
                          .author("Blake Kaplan <mrbkap@gmail.com>")
                          .about("Parses mochitest browser-chrome logs to find CPOW uses")
                          .args_from_usage("[shims] -s, --include-shims   'Specifies whether to include CPOWs via shims'
                                            <FILES>...                    'The log files to parse (\"-\" to specify stdin)'")
                          .get_matches();

    let include_shims = matches.is_present("shims");
    if include_shims {
        // Empty line intentional.
        println!("Including CPOWs via shims. Indirect CPOWs via a shim are marked by a leading *.\n");
    }

    let m = Parser::new();
    let mut all_tests = BTreeMap::new();
    let mut num_cpows = 0;

    for fname in matches.values_of("FILES").unwrap() {
        let p = m.parse_file(&fname);
        let tests = CPOWFinder::compile_cpows(p.as_slice(), include_shims);

        for test in tests {
            num_cpows += test.cpows.len() + test.indirect_cpows.len();
            all_tests.insert(test.testname.clone(), test);
        }
    }

    println!("Found {} CPOWs in {} tests", num_cpows, all_tests.len());
    for (_, test) in &all_tests {
        print!("{} -", test.testname);
        let mut non_shims = test.cpows.iter()
                                      .filter(|c| !c.shim)
                                      .map(|c| c.line_no)
                                      .collect::<Vec<_>>();
        let mut shims = test.cpows.iter()
                                  .filter(|c| c.shim)
                                  .map(|c| c.line_no)
                                  .collect::<Vec<_>>();

        non_shims.dedup();
        shims.dedup();

        if !non_shims.is_empty() {
            print!(" {:?}", non_shims);
        }
        if !shims.is_empty() {
            print!(" shims: {:?}", shims);
        }
        println!("");

        if !test.indirect_cpows.is_empty() {
            let mut last_lineno = 0u32;
            for icpow in test.indirect_cpows.iter() {
                if icpow.line_no != last_lineno {
                    last_lineno = icpow.line_no;
                    println!("\t{} {}:{}", if icpow.shim { "*" } else { " " }, icpow.filename, icpow.line_no);
                }
            }
        }
    }
}
