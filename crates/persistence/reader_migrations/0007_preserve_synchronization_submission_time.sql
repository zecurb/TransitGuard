ALTER TABLE synchronization_batches
ADD COLUMN submitted_at_unix_milliseconds INTEGER
    CHECK (
        submitted_at_unix_milliseconds IS NULL
        OR submitted_at_unix_milliseconds >= 0
    );
