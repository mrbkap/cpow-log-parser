extern crate regex;

use std::collections::HashSet;
use std::env::args;
use std::fs::File;
use std::io::{BufRead, BufReader};

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

#[derive(Debug)]
struct Test {
    testname: String,
    cpows: Vec<CPOW>,
}

#[derive(Debug)]
enum LogLine {
    TestStart(String),
    StackComponent(u32, String, u32),
}

impl Parser {
    fn new() -> Parser {
        Parser {
            test_start: Regex::new(r"\bTEST-START\s+\|\s+.+/(.+)$").unwrap(),
            // Capture indices:              1                                    2     3
            stack_component: Regex::new(r"#(\d+)\s+0x[0-9a-zA-Z]{12}\s+[ib]\s+.+/(.+):(\d+)\s+\(.*\)$").unwrap(),
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
                let fname = String::from(captures.at(2).unwrap());
                let line_no = captures.at(3).unwrap().parse::<u32>().unwrap();
                parsed.push(LogLine::StackComponent(idx, fname, line_no));
            }
        }

        parsed
    }
}

struct CPOWFinder<'a> {
    idx: usize,
    lines: &'a [LogLine],
    peeked: bool,
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
    fn parse_cpow(&mut self, testname: &str) -> Option<CPOW> {
        let mut report = true; // only report CPOWs from this test.
        let mut cpow = match self.next_line() {
            &LogLine::StackComponent(idx, ref filename, line_no) => {
                assert!(idx == 0);
                if filename != testname {
                    report = false;
                }

                CPOW {
                    line_no: line_no,
                    shim: false
                }
            }
            &LogLine::TestStart(_) => {
                panic!("bad line");
            }
        };

        while let Some(next_line) = self.peek_line() {
            match next_line {
                // Pull lines until we find the next test or the next CPOW.
                &LogLine::StackComponent(0, _, _) | &LogLine::TestStart(_) => break,
                &LogLine::StackComponent(_, ref filename, _) => {
                    self.take_line();
                    if !cpow.shim && filename == "RemoteAddonsParent.jsm" {
                        cpow.shim = true;
                    }
                }
            }
        }

        if report { Some(cpow) } else { None }
    }

    // Given a TEST-START, looks for and accumulates CPOW uses.
    fn parse_test(&mut self, testname: &str) -> Option<Test> {
        let mut cpows = Vec::new();
        while let Some(next_line) = self.peek_line() {
            match next_line {
                &LogLine::TestStart(_) => break,
                &LogLine::StackComponent(_, _, _) => {
                    if let Some(c) = self.parse_cpow(testname) {
                        cpows.push(c);
                    }
                }
            }
        }

        if !cpows.is_empty() {
            cpows.sort_by_key(|k| k.line_no);
            Some(Test { testname: String::from(testname), cpows: cpows })
        } else {
            None
        }
    }

    // Returns a list of tests that have CPOW uses.
    fn compile_cpows(lines: &[LogLine]) -> Vec<Test> {
        let mut finder = CPOWFinder { idx: 0, lines: lines, peeked: false };
        let mut tests = Vec::new();
        while let Some(next_line) = finder.peek_line() {
            match next_line {
                &LogLine::StackComponent(..) => continue,
                &LogLine::TestStart(ref fname) => {
                    finder.take_line();
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
    let m = Parser::new();

    for fname in args() {
        let p = m.parse_file(&fname);
        let tests = CPOWFinder::compile_cpows(p.as_slice());

        for test in tests {
            println!("{} -", test.testname);

            // Only print each line's CPOW once.
            let mut h = HashSet::<u32>::new();
            for c in test.cpows {
                if !h.contains(&c.line_no) {
                    h.insert(c.line_no);
                    println!("\t{} ({})", c.line_no, c.shim);
                }
            }
        }
    }
}
