package ai.chat2db.rust.compat;

import java.util.ArrayList;
import java.util.Comparator;
import java.util.LinkedHashSet;
import java.util.List;

/** Exact-value redaction for connection properties explicitly marked sensitive. */
final class SensitiveDataRedactor {

    static final SensitiveDataRedactor NONE = new SensitiveDataRedactor(List.of());

    private static final String REDACTED = "[REDACTED]";
    private final List<String> values;
    private final int longestValueLength;

    SensitiveDataRedactor(List<String> requestedValues) {
        LinkedHashSet<String> unique = new LinkedHashSet<>();
        for (String value : requestedValues) {
            if (value != null && !value.isEmpty()) {
                unique.add(value);
            }
        }
        ArrayList<String> ordered = new ArrayList<>(unique);
        ordered.sort(Comparator.comparingInt(String::length).reversed());
        values = List.copyOf(ordered);
        longestValueLength = values.isEmpty() ? 0 : values.get(0).length();
    }

    String redactAndTruncate(CharSequence input, int maximumBytes) {
        if (input == null) {
            return null;
        }
        if (input.length() == 0 || maximumBytes <= 0) {
            return "";
        }

        StringBuilder output = new StringBuilder(Math.min(input.length(), maximumBytes));
        int inputStartLimit = Math.min(input.length(), maximumBytes);
        long maximumComparisons = (long) maximumBytes * Math.max(1, values.size())
                + longestValueLength;
        ComparisonBudget comparisonBudget = new ComparisonBudget(maximumComparisons);
        int outputBytes = 0;
        int offset = 0;
        while (offset < inputStartLimit && outputBytes < maximumBytes) {
            String matched = matchingValue(input, offset, comparisonBudget);
            if (comparisonBudget.exhausted()) {
                break;
            }
            if (matched != null) {
                int available = maximumBytes - outputBytes;
                int markerLength = Math.min(REDACTED.length(), available);
                output.append(REDACTED, 0, markerLength);
                outputBytes += markerLength;
                offset += matched.length();
                if (markerLength < REDACTED.length()) {
                    break;
                }
                continue;
            }

            char first = input.charAt(offset);
            int charCount = 1;
            int encodedBytes;
            if (Character.isHighSurrogate(first)
                    && offset + 1 < input.length()
                    && Character.isLowSurrogate(input.charAt(offset + 1))) {
                charCount = 2;
                encodedBytes = 4;
            } else if (first <= 0x7f) {
                encodedBytes = 1;
            } else if (first <= 0x7ff) {
                encodedBytes = 2;
            } else {
                encodedBytes = 3;
            }
            if (outputBytes > maximumBytes - encodedBytes) {
                break;
            }
            output.append(first);
            if (charCount == 2) {
                output.append(input.charAt(offset + 1));
            }
            outputBytes += encodedBytes;
            offset += charCount;
        }
        return output.toString();
    }

    private String matchingValue(
            CharSequence input, int offset, ComparisonBudget comparisonBudget) {
        if (values.isEmpty()) {
            return null;
        }
        char first = input.charAt(offset);
        for (String value : values) {
            if (value.charAt(0) != first || value.length() > input.length() - offset) {
                continue;
            }
            boolean matches = true;
            for (int index = 1; index < value.length(); index++) {
                if (!comparisonBudget.consume()) {
                    return null;
                }
                if (input.charAt(offset + index) != value.charAt(index)) {
                    matches = false;
                    break;
                }
            }
            if (matches) {
                return value;
            }
        }
        return null;
    }

    private static final class ComparisonBudget {
        private long remaining;
        private boolean exhausted;

        private ComparisonBudget(long remaining) {
            this.remaining = remaining;
        }

        private boolean consume() {
            if (remaining == 0) {
                exhausted = true;
                return false;
            }
            remaining--;
            return true;
        }

        private boolean exhausted() {
            return exhausted;
        }
    }
}
