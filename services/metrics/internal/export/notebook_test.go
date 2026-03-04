package export

import (
	"encoding/json"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/org/experimentation-platform/services/metrics/internal/querylog"
)

func TestGenerateNotebook_ValidJSON(t *testing.T) {
	entries := []querylog.Entry{
		{
			ExperimentID: "exp-001",
			MetricID:     "watch_time",
			SQLText:      "SELECT AVG(value) FROM metric_data GROUP BY user_id",
			RowCount:     1000,
			DurationMs:   250,
			JobType:      "daily_metric",
		},
		{
			ExperimentID: "exp-001",
			MetricID:     "ctr",
			SQLText:      "SELECT CASE WHEN COUNT(*) > 0 THEN 1.0 ELSE 0.0 END FROM metric_data",
			RowCount:     500,
			DurationMs:   150,
			JobType:      "daily_metric",
		},
	}

	data, err := GenerateNotebook("exp-001", entries)
	require.NoError(t, err)

	// Must be valid JSON.
	var parsed map[string]any
	err = json.Unmarshal(data, &parsed)
	require.NoError(t, err, "notebook output must be valid JSON")

	// Verify nbformat fields.
	assert.Equal(t, float64(4), parsed["nbformat"])
	assert.Equal(t, float64(5), parsed["nbformat_minor"])
}

func TestGenerateNotebook_Structure(t *testing.T) {
	entries := []querylog.Entry{
		{
			ExperimentID: "exp-001",
			MetricID:     "metric_a",
			SQLText:      "SELECT 1",
			RowCount:     10,
			DurationMs:   5,
			JobType:      "daily_metric",
		},
	}

	data, err := GenerateNotebook("exp-001", entries)
	require.NoError(t, err)

	var nb Notebook
	err = json.Unmarshal(data, &nb)
	require.NoError(t, err)

	// Expected cells: header markdown + setup code + (description markdown + SQL code) per entry
	assert.Equal(t, 4, len(nb.Cells))

	// Cell 0: header markdown
	assert.Equal(t, "markdown", nb.Cells[0].CellType)
	assert.Contains(t, nb.Cells[0].Source[0], "exp-001")

	// Cell 1: setup code
	assert.Equal(t, "code", nb.Cells[1].CellType)
	assert.Contains(t, nb.Cells[1].Source[0], "SparkSession")

	// Cell 2: metric description markdown
	assert.Equal(t, "markdown", nb.Cells[2].CellType)
	assert.Contains(t, nb.Cells[2].Source[0], "metric_a")

	// Cell 3: SQL code cell
	assert.Equal(t, "code", nb.Cells[3].CellType)
	assert.Contains(t, nb.Cells[3].Source[0], "SELECT 1")
}

func TestGenerateNotebook_CellFields(t *testing.T) {
	entries := []querylog.Entry{
		{
			ExperimentID: "exp-001",
			MetricID:     "m1",
			SQLText:      "SELECT 1",
			JobType:      "daily_metric",
		},
	}

	data, err := GenerateNotebook("exp-001", entries)
	require.NoError(t, err)

	var nb Notebook
	err = json.Unmarshal(data, &nb)
	require.NoError(t, err)

	for i, cell := range nb.Cells {
		assert.NotEmpty(t, cell.CellType, "cell %d must have cell_type", i)
		assert.NotEmpty(t, cell.Source, "cell %d must have source", i)
		assert.NotNil(t, cell.Metadata, "cell %d must have metadata", i)
	}

	// Verify the raw JSON has "outputs" key in code cells.
	var rawNB map[string]any
	err = json.Unmarshal(data, &rawNB)
	require.NoError(t, err)
	rawCells := rawNB["cells"].([]any)
	for i, rawCell := range rawCells {
		cellMap := rawCell.(map[string]any)
		if cellMap["cell_type"] == "code" {
			_, hasOutputs := cellMap["outputs"]
			assert.True(t, hasOutputs, "code cell %d must have 'outputs' key in JSON", i)
		}
	}
}

func TestGenerateNotebook_KernelSpec(t *testing.T) {
	data, err := GenerateNotebook("exp-001", []querylog.Entry{
		{ExperimentID: "exp-001", MetricID: "m1", SQLText: "SELECT 1", JobType: "daily_metric"},
	})
	require.NoError(t, err)

	var nb Notebook
	err = json.Unmarshal(data, &nb)
	require.NoError(t, err)

	assert.Equal(t, "python3", nb.Metadata.KernelSpec.Name)
	assert.Equal(t, "python", nb.Metadata.KernelSpec.Language)
}
