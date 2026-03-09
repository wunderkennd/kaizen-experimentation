package spark

import (
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestValidateCustomSQL_ValidQueries(t *testing.T) {
	valid := []string{
		"SELECT user_id, AVG(value) AS metric_value FROM events GROUP BY user_id",
		"WITH cte AS (SELECT * FROM events) SELECT user_id, SUM(value) AS metric_value FROM cte GROUP BY user_id",
		"select user_id, count(*) as metric_value from events group by user_id",
		"SELECT user_id, CASE WHEN cnt >= 10 THEN avg_val ELSE 0 END AS metric_value FROM (SELECT user_id, COUNT(*) AS cnt, AVG(value) AS avg_val FROM events GROUP BY user_id)",
	}
	for _, sql := range valid {
		t.Run(sql[:min(len(sql), 40)], func(t *testing.T) {
			err := ValidateCustomSQL(sql)
			assert.NoError(t, err)
		})
	}
}

func TestValidateCustomSQL_EmptySQL(t *testing.T) {
	err := ValidateCustomSQL("")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "must not be empty")

	err = ValidateCustomSQL("   ")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "must not be empty")
}

func TestValidateCustomSQL_ForbiddenStatements(t *testing.T) {
	tests := []struct {
		name string
		sql  string
		want string
	}{
		{"DROP", "DROP TABLE events", "must start with SELECT"},
		{"CREATE", "CREATE TABLE foo (id INT)", "must start with SELECT"},
		{"INSERT_prefix", "INSERT INTO events VALUES (1)", "must start with SELECT"},
		{"DELETE_prefix", "DELETE FROM events", "must start with SELECT"},
		{"SELECT_with_DROP", "SELECT 1; DROP TABLE events", "forbidden statement \"DROP\""},
		{"SELECT_with_DELETE", "SELECT * FROM events WHERE DELETE = 1", "forbidden statement \"DELETE\""},
		{"SELECT_with_INSERT", "SELECT * FROM (INSERT INTO foo VALUES (1))", "forbidden statement \"INSERT\""},
		{"SELECT_with_UPDATE", "SELECT * FROM events; UPDATE events SET value = 0", "forbidden statement \"UPDATE\""},
		{"SELECT_with_TRUNCATE", "SELECT * FROM events; TRUNCATE TABLE events", "forbidden statement \"TRUNCATE\""},
		{"SELECT_with_ALTER", "SELECT * FROM events; ALTER TABLE events ADD col INT", "forbidden statement \"ALTER\""},
		{"SELECT_with_GRANT", "SELECT 1; GRANT ALL ON events TO user", "forbidden statement \"GRANT\""},
		{"SELECT_with_MERGE", "SELECT 1; MERGE INTO t USING s ON t.id = s.id", "forbidden statement \"MERGE\""},
		{"case_insensitive", "SELECT 1; drop table events", "forbidden statement \"DROP\""},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			err := ValidateCustomSQL(tc.sql)
			require.Error(t, err)
			assert.Contains(t, err.Error(), tc.want)
		})
	}
}

func TestValidateCustomSQL_MustStartWithSelect(t *testing.T) {
	err := ValidateCustomSQL("EXPLAIN SELECT 1")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "must start with SELECT or WITH")
}

func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}
