## About
Parses `mochitest-browser-chrome` logs generated with a patch (sample below)
to show CPOWs used by those tests.

To build, you must have a [Rust](https://www.rust-lang.org/en-US/) compiler
installed.

Sample usage assuming tests have been downloaded locally to
`~/tmp/cpow-logs/`:

    $ cargo build --release
    $ cargo run --release -- -s ~/tmp/cpow-logs/*.log
    <output is the CPOWs>


This will find CPOW uses that come through shims as well as those that are in
the test (e.g. `browser.contentDocument.location`). It reports "indirect"
CPOWS (i.e. CPOW uses that happen in shared `head.js` files) via their full
filenames but will prefer to show the line in the test file that uses the
CPOW. Each line is reported only once, therefore a helper function that relies
on CPOWs will only appear once in the output.

## Generating logs

To generate logs, apply something like this patch:

```diff
diff --git a/js/ipc/JavaScriptShared.cpp b/js/ipc/JavaScriptShared.cpp
index 7af8adb..97667e7 100644
--- a/js/ipc/JavaScriptShared.cpp
+++ b/js/ipc/JavaScriptShared.cpp
@@ -169,7 +169,7 @@ JavaScriptShared::JavaScriptShared()
             Preferences::AddBoolVarCache(&sLoggingEnabled,
                                          "dom.ipc.cpows.log.enabled", false);
             Preferences::AddBoolVarCache(&sStackLoggingEnabled,
-                                         "dom.ipc.cpows.log.stack", false);
+                                         "dom.ipc.cpows.log.stack", true);
        }
    }
}
```

and push the result to try. I then manually download the `m-e10s(bc*)` logs
and run the sample command, above.
