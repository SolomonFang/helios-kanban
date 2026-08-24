-- Add type field to tasks, following conventional commit types.
-- Eight types: 'feat' | 'fix' | 'docs' | 'style' | 'refactor' | 'perf' | 'test' | 'chore'. Defaults to 'feat'.

ALTER TABLE tasks ADD COLUMN task_type TEXT NOT NULL DEFAULT 'feat'
    CHECK (task_type IN ('feat','fix','docs','style','refactor','perf','test','chore'));
