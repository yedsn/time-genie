-- Timer start/transition now include ordinary source tasks in the current
-- work day's Today plan. The direct-v2 timer definitions are idempotent, so
-- reapplying the complete timer patch upgrades existing deployments safely.
\ir 20261022k_timegenie_direct_cloud_timer_mutations.sql
