extern crate clap;
extern crate regex;

use std::fs::File;
use std::io::{BufRead, BufReader};
use clap::App;

use regex::Regex;

struct Parser {
    test_start: Regex,
    stack_component: Regex,
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
        Parser {
            test_start: Regex::new(r"\bTEST-START\s+\|\s+.+/(.+)$").unwrap(),
            // Capture indices:              1                                    2   3     4
            stack_component: Regex::new(r"#(\d+)\s+0x[0-9a-zA-Z]{8,12}\s+[ib]\s+(.+/(.+)):(\d+)\s+\(.*\)$").unwrap(),
        }
    }

    fn parse_file(self: &Parser, fname: &str) -> Vec<LogLine> {
        let mut parsed = Vec::new();
        let file = File::open(fname);
        if file.is_err() {
            println!("Warning ({}): {}", fname, file.unwrap_err());
            return parsed;
        }
        let f = BufReader::new(file.unwrap());
        for line in f.lines().filter_map(|r| r.ok()) {
            if let Some(captures) = self.test_start.captures(&line) {
                let testname = String::from(captures.at(1).unwrap());
                parsed.push(LogLine::TestStart(testname));
            } else if let Some(captures) = self.stack_component.captures(&line) {
                let idx: u32 = captures.at(1).unwrap().parse::<u32>().unwrap();
                let path = String::from(captures.at(2).unwrap());
                let fname = String::from(captures.at(3).unwrap());
                let line_no = captures.at(4).unwrap().parse::<u32>().unwrap();
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
            p.starts_with("chrome://mochitests/")
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
                                            <FILES>...                    'The log files to parse'")
                          .get_matches();

    let include_shims = matches.is_present("shims");
    if include_shims {
        // Empty line intentional.
        println!("Including CPOWs via shims. Indirect CPOWs via a shim are marked by a leading *.\n");
    }

    let m = Parser::new();

    for fname in matches.values_of("FILES").unwrap() {
        let p = m.parse_file(&fname);
        let tests = CPOWFinder::compile_cpows(p.as_slice(), include_shims);

        for test in tests {
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
                let mut last_lineno = 0;
                for icpow in test.indirect_cpows.iter() {
                    if icpow.line_no != last_lineno {
                        last_lineno = icpow.line_no;
                        println!("\t{} {}:{}", if icpow.shim { "*" } else { " " }, icpow.filename, icpow.line_no);
                    }
                }
            }
        }
    }
}
