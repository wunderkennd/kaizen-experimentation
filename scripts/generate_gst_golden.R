#!/usr/bin/env Rscript
# Generate GST golden files from R's gsDesign package.
#
# These golden files validate our Rust gst_boundaries() implementation
# against the Armitage-McPherson-Rowe recursive integration used by gsDesign.
#
# Usage: Rscript scripts/generate_gst_golden.R
# Requires: install.packages(c("gsDesign", "jsonlite"))

suppressPackageStartupMessages({
  library(gsDesign)
  library(jsonlite)
})

output_dir <- "crates/experimentation-stats/tests/golden"
if (!dir.exists(output_dir)) dir.create(output_dir, recursive = TRUE)

# Configuration: each entry generates one golden file.
# test.type=2 => symmetric two-sided boundaries.
# sfu: sfLDOF = Lan-DeMets O'Brien-Fleming, sfLDPocock = Lan-DeMets Pocock
configs <- list(
  list(name = "gst_obf_4_looks",         k = 4, alpha = 0.05, sfu = "sfLDOF",    sf_label = "OBrienFleming"),
  list(name = "gst_pocock_4_looks",       k = 4, alpha = 0.05, sfu = "sfLDPocock", sf_label = "Pocock"),
  list(name = "gst_obf_5_looks_alpha10",  k = 5, alpha = 0.10, sfu = "sfLDOF",    sf_label = "OBrienFleming"),
  list(name = "gst_pocock_3_looks",       k = 3, alpha = 0.05, sfu = "sfLDPocock", sf_label = "Pocock"),
  list(name = "gst_obf_2_looks",          k = 2, alpha = 0.05, sfu = "sfLDOF",    sf_label = "OBrienFleming"),
  list(name = "gst_pocock_2_looks",       k = 2, alpha = 0.05, sfu = "sfLDPocock", sf_label = "Pocock"),
  list(name = "gst_obf_6_looks",          k = 6, alpha = 0.05, sfu = "sfLDOF",    sf_label = "OBrienFleming"),
  list(name = "gst_pocock_6_looks",       k = 6, alpha = 0.05, sfu = "sfLDPocock", sf_label = "Pocock"),
  list(name = "gst_obf_3_looks_alpha01",  k = 3, alpha = 0.01, sfu = "sfLDOF",    sf_label = "OBrienFleming"),
  list(name = "gst_pocock_5_looks_alpha10", k = 5, alpha = 0.10, sfu = "sfLDPocock", sf_label = "Pocock")
)

for (cfg in configs) {
  sfu_fn <- get(cfg$sfu)
  r_cmd <- sprintf("gsDesign(k=%d, test.type=2, alpha=%g, sfu=%s)", cfg$k, cfg$alpha, cfg$sfu)

  d <- gsDesign(k = cfg$k, test.type = 2, alpha = cfg$alpha, sfu = sfu_fn)

  # d$upper$bound = critical z-values at each look
  # d$upper$spend = incremental one-sided alpha spent at each look
  # d$timing      = information fractions (equally spaced by default)

  boundaries <- list()
  cum_alpha <- 0.0
  for (i in seq_len(cfg$k)) {
    # gsDesign $upper$spend is the incremental one-sided spend.
    # Our convention is two-sided: multiply by 2.
    inc_alpha_twosided <- d$upper$spend[i] * 2.0
    cum_alpha <- cum_alpha + inc_alpha_twosided

    boundaries[[i]] <- list(
      look = i,
      information_fraction = d$timing[i],
      cumulative_alpha = cum_alpha,
      incremental_alpha = inc_alpha_twosided,
      critical_value = d$upper$bound[i]
    )
  }

  golden <- list(
    test_name = cfg$name,
    spending_function = cfg$sf_label,
    planned_looks = cfg$k,
    overall_alpha = cfg$alpha,
    source = "gsDesign",
    r_command = r_cmd,
    r_version = paste0("gsDesign ", packageVersion("gsDesign")),
    boundaries = boundaries
  )

  outfile <- file.path(output_dir, paste0(cfg$name, ".json"))
  write_json(golden, outfile, pretty = TRUE, auto_unbox = TRUE, digits = 15)
  cat(sprintf("Wrote %s (%d looks, alpha=%.2f, %s)\n", outfile, cfg$k, cfg$alpha, cfg$sf_label))
}

cat(sprintf("\nGenerated %d golden files in %s/\n", length(configs), output_dir))
