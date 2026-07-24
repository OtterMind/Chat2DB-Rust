package ai.chat2db.rust.compat;

import java.io.PrintStream;

/** Process entrypoint for the private Java database compatibility engine. */
public final class CompatibilityRuntime {

    private CompatibilityRuntime() {
    }

    public static void main(String[] args) {
        int exitCode = run(args, System.out, System.err);
        if (exitCode != 0) {
            System.exit(exitCode);
        }
    }

    static int run(String[] args, PrintStream standardOutput, PrintStream standardError) {
        if (args.length == 1 && "--version".equals(args[0])) {
            standardOutput.println(RuntimeInfo.current().displayVersion());
            return 0;
        }

        standardError.println("The compatibility protocol is not enabled in the bootstrap build.");
        return 2;
    }
}
