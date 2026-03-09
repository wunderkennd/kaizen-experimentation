package spark

import (
	"fmt"
	"regexp"
	"strings"
)

// forbiddenPatterns matches DDL/DML statements that CUSTOM SQL must not contain.
// Custom metrics are restricted to read-only SELECT queries.
var forbiddenPatterns = regexp.MustCompile(
	`(?i)\b(CREATE|DROP|ALTER|TRUNCATE|DELETE|INSERT|UPDATE|MERGE|GRANT|REVOKE|CALL)\b`,
)

// ValidateCustomSQL checks that user-provided SQL is safe for execution.
// It rejects DDL/DML statements and requires the SQL to start with SELECT or WITH.
func ValidateCustomSQL(sql string) error {
	trimmed := strings.TrimSpace(sql)
	if trimmed == "" {
		return fmt.Errorf("custom SQL must not be empty")
	}

	upper := strings.ToUpper(trimmed)
	if !strings.HasPrefix(upper, "SELECT") && !strings.HasPrefix(upper, "WITH") {
		return fmt.Errorf("custom SQL must start with SELECT or WITH, got: %q", truncate(trimmed, 40))
	}

	if loc := forbiddenPatterns.FindStringIndex(sql); loc != nil {
		keyword := sql[loc[0]:loc[1]]
		return fmt.Errorf("custom SQL contains forbidden statement %q — only SELECT queries are allowed", strings.ToUpper(keyword))
	}

	return nil
}

func truncate(s string, n int) string {
	if len(s) <= n {
		return s
	}
	return s[:n] + "..."
}
