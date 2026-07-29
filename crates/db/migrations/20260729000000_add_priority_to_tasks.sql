-- Add priority field to tasks.
-- Four levels: 'urgent' | 'high' | 'medium' | 'low'. Defaults to 'medium'.

ALTER TABLE tasks ADD COLUMN priority TEXT NOT NULL DEFAULT 'medium'
    CHECK (priority IN ('urgent','high','medium','low'));
