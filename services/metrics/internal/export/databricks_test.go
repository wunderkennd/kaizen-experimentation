package export

import (
	"strings"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
)

func TestGenerateDatabricksNotebook_Header(t *testing.T) {
	data, err := GenerateDatabricksNotebook("exp-001", []querylog.Entry{
		{ExperimentID: "exp-001", MetricID: "m1", SQLText: "SELECT 1", JobType: "daily_metric"},
	})
	require.NoError(t, err)

	content := string(data)
	assert.True(t, strings.HasPrefix(content, "# Databricks notebook source"),
		"must start with Databricks header")
}

func TestGenerateDatabricksNotebook_Structure(t *testing.T) {
	entries := []querylog.Entry{
		{ExperimentID: "exp-001", MetricID: "watch_time", SQLText: "SELECT AVG(value) FROM events", RowCount: 1000, DurationMs: 250, JobType: "daily_metric"},
		{ExperimentID: "exp-001", MetricID: "ctr", SQLText: "SELECT SUM(clicks)/SUM(impressions) FROM events", RowCount: 500, DurationMs: 150, JobType: "daily_metric"},
	}

	data, err := GenerateDatabricksNotebook("exp-001", entries)
	require.NoError(t, err)
	content := string(data)

	// Verify COMMAND delimiters: header→setup + 2 entries × (markdown + code) = 1 + 2*2 = 5 delimiters
	assert.Equal(t, 5, strings.Count(content, "# COMMAND ----------"))

	// Verify markdown magic lines exist for both metrics.
	assert.Contains(t, content, "# MAGIC ## Metric: watch_time")
	assert.Contains(t, content, "# MAGIC ## Metric: ctr")

	// Verify SQL is embedded in code cells.
	assert.Contains(t, content, "SELECT AVG(value) FROM events")
	assert.Contains(t, content, "SELECT SUM(clicks)/SUM(impressions) FROM events")

	// Verify display() is used (Databricks-native) instead of .show().
	assert.Contains(t, content, "display(df)")
}

func TestGenerateDatabricksNotebook_MetadataFields(t *testing.T) {
	entries := []querylog.Entry{
		{ExperimentID: "exp-001", MetricID: "m1", SQLText: "SELECT 1", RowCount: 42, DurationMs: 10, JobType: "hourly_guardrail"},
	}

	data, err := GenerateDatabricksNotebook("exp-001", entries)
	require.NoError(t, err)
	content := string(data)

	assert.Contains(t, content, "# MAGIC - **Job type**: hourly_guardrail")
	assert.Contains(t, content, "# MAGIC - **Row count**: 42")
	assert.Contains(t, content, "# MAGIC - **Duration**: 10 ms")
}

func TestGenerateDatabricksNotebook_ExperimentIDInTitle(t *testing.T) {
	data, err := GenerateDatabricksNotebook("e0000000-0000-0000-0000-000000000001", []querylog.Entry{
		{ExperimentID: "e0000000-0000-0000-0000-000000000001", MetricID: "m1", SQLText: "SELECT 1", JobType: "daily_metric"},
	})
	require.NoError(t, err)
	content := string(data)

	assert.Contains(t, content, "# MAGIC # Experiment Analysis: e0000000-0000-0000-0000-000000000001")
}

func TestGenerateDatabricksNotebook_EmptyEntries(t *testing.T) {
	data, err := GenerateDatabricksNotebook("exp-001", []querylog.Entry{})
	require.NoError(t, err)
	content := string(data)

	// Should still have the header and setup cell.
	assert.Contains(t, content, "# Databricks notebook source")
	assert.Contains(t, content, "SparkSession")

	// Only 1 COMMAND delimiter (between title and setup).
	assert.Equal(t, 1, strings.Count(content, "# COMMAND ----------"))
}

func TestGenerateDatabricksNotebook_SQLWithSpecialChars(t *testing.T) {
	sql := "SELECT user_id, SUM(CASE WHEN event = 'click' THEN 1 ELSE 0 END) AS clicks\nFROM events\nWHERE experiment_id = 'exp-001'\nGROUP BY user_id"
	entries := []querylog.Entry{
		{ExperimentID: "exp-001", MetricID: "clicks", SQLText: sql, JobType: "daily_metric"},
	}

	data, err := GenerateDatabricksNotebook("exp-001", entries)
	require.NoError(t, err)
	content := string(data)

	// SQL should be embedded verbatim within triple quotes.
	assert.Contains(t, content, sql)
}

func TestGenerateDatabricksNotebook_SetupCellCommented(t *testing.T) {
	data, err := GenerateDatabricksNotebook("exp-001", []querylog.Entry{
		{ExperimentID: "exp-001", MetricID: "m1", SQLText: "SELECT 1", JobType: "daily_metric"},
	})
	require.NoError(t, err)
	content := string(data)

	// SparkSession import should be commented out (Databricks provides spark automatically).
	assert.Contains(t, content, "# from pyspark.sql import SparkSession")
	assert.Contains(t, content, "# spark = SparkSession.builder")
}
