extern crate regex;

use std::io::{BufRead, BufReader};
use std::fs::File;

use regex::Regex;

struct Matcher {
    test_start: Regex,
    stack_component: Regex,
}

struct CPOW {
    line_no: u32,
    shim: bool,
}

struct Test {
    testname: String,
    cpows: Vec<CPOW>,
}

impl Matcher {
    fn new() -> Matcher {
        Matcher {
            test_start: Regex::new(r"\bTEST-START\s+\|\s+(.+)$").unwrap(),
            //                               1                                  2   3     4
            stack_component: Regex::new(r"#(\d+)\s+0x[0-9a-zA-Z]{12}\s+[ib]\s+(.+)/(.+):(\d+)\s+\(.*\)").unwrap(),
        }
    }

    fn parse_file(self: &Matcher, fname: &str) -> Vec<Test> {
        let mut tests = Vec::new();
        let mut file = File::open(fname).unwrap();
        let mut f = BufReader::new(file);
        let mut testname: String;
        let mut cur_test: Option<Test> = None;
        let mut cur_cpow: Option<CPOW> = None;
        for line in f.lines().filter_map(|r| r.ok()) {
            if let Some(captures) = self.test_start.captures(&line) {
                if let Some(cpow) = cur_cpow {
                    match cur_test {
                        Some(mut t) => { t.cpows.push(cpow); }
                        None => { tests.push(Test { testname: testname, cpows: vec![ cpow ] }); }
                    }
                }
                if let Some(mut t) = cur_test {
                    tests.push(t);
                }

                cur_test = None;
                cur_cpow = None;
                testname = String::from(captures.at(1).unwrap());
            } else if let Some(captures) = self.stack_component.captures(&line) {
                let idx: u32 = captures.at(1).unwrap().parse::<u32>().unwrap();
                let fname = captures.at(3).unwrap();
                let line_no = captures.at(4).unwrap().parse::<u32>().unwrap();
                if idx == 0 {
                    if let Some(cpow) = cur_cpow {
                        let mut test = cur_test.unwrap();
                        test.cpows.push(cpow);
                    }

                    let cur_line = if fname == testname { line_no } else { 0 };
                    cur_cpow = Some(CPOW { line_no: cur_line, shim: false });

                    if cur_test.is_none() {
                        cur_test = Some(Test { testname: testname, cpows: Vec::new() });
                    }
                } else {
                    // todo
                }
            }
        }

        tests
    }
}

fn main() {
    let m = Matcher::new();
    println!("{:?}", m.test_start.find("[task 2016-10-04T23:50:30.410193Z] 23:50:30     INFO -  MEMORY STAT | vsize 1116MB | residentFast 264MB | heapAllocated 118MB"));
    let res = m.test_start.captures("[task 2016-10-04T23:50:30.073593Z] 23:50:30     INFO -  42 INFO TEST-START | browser/components/search/test/browser_addEngine.js");
    println!("{:?}", res);
    /*
    for cap in res.captures_iter() {
        println("{:?}", cap);
    }*/
    println!("{:?}", m.stack_component.find("[task 2016-10-04T23:50:30.073593Z] 23:50:30     INFO -  42 INFO TEST-START | browser/components/search/test/browser_addEngine.js"));
    println!("{:?}", m.stack_component.captures("[task 2016-10-04T23:50:31.304736Z] 23:50:31     INFO -  #0 0x7faaa0c09198 i   chrome://mochitests/content/browser/browser/components/search/test/browser_amazon_behavior.js:136 (0x7faa865989a0 @ 80)"));
}
