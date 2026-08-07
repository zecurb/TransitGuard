-- Rebuild the acknowledgement-entry table with explicit NULL-safe,
-- state-aware failure metadata constraints.
--
-- SQLite treats a CHECK expression that evaluates to NULL as successful.
-- Failure categories therefore require explicit IS NOT NULL predicates.

CREATE TABLE synchronization_acknowledgement_entries_v3 (
    batch_id TEXT NOT NULL
        CHECK (length(batch_id) = 36),

    reader_id TEXT NOT NULL
        CHECK (length(reader_id) = 36),

    fare_transaction_id TEXT NOT NULL
        CHECK (length(fare_transaction_id) = 36),

    local_sequence_number INTEGER NOT NULL
        CHECK (local_sequence_number > 0),

    entry_position INTEGER NOT NULL
        CHECK (entry_position >= 0),

    outcome TEXT NOT NULL
        CHECK (
            outcome IN (
                'acknowledged',
                'retryable_failure',
                'permanent_failure',
                'manual_review'
            )
        ),

    failure_category TEXT,

    retry_at_unix_milliseconds INTEGER
        CHECK (
            retry_at_unix_milliseconds IS NULL
            OR retry_at_unix_milliseconds >= 0
        ),

    PRIMARY KEY (
        batch_id,
        fare_transaction_id
    ),

    UNIQUE (
        batch_id,
        local_sequence_number
    ),

    UNIQUE (
        batch_id,
        entry_position
    ),

    CHECK (
        (
            outcome = 'acknowledged'
            AND failure_category IS NULL
            AND retry_at_unix_milliseconds IS NULL
        )
        OR (
            outcome = 'retryable_failure'
            AND failure_category IS NOT NULL
            AND length(trim(failure_category)) > 0
            AND retry_at_unix_milliseconds IS NOT NULL
        )
        OR (
            outcome IN (
                'permanent_failure',
                'manual_review'
            )
            AND failure_category IS NOT NULL
            AND length(trim(failure_category)) > 0
            AND retry_at_unix_milliseconds IS NULL
        )
    ),

    FOREIGN KEY (
        batch_id,
        reader_id
    )
        REFERENCES synchronization_acknowledgements (
            batch_id,
            reader_id
        )
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    FOREIGN KEY (
        batch_id,
        fare_transaction_id
    )
        REFERENCES synchronization_entries (
            batch_id,
            fare_transaction_id
        )
        ON UPDATE RESTRICT
        ON DELETE RESTRICT
);

INSERT INTO synchronization_acknowledgement_entries_v3 (
    batch_id,
    reader_id,
    fare_transaction_id,
    local_sequence_number,
    entry_position,
    outcome,
    failure_category,
    retry_at_unix_milliseconds
)
SELECT
    batch_id,
    reader_id,
    fare_transaction_id,
    local_sequence_number,
    entry_position,
    outcome,
    failure_category,
    retry_at_unix_milliseconds
FROM synchronization_acknowledgement_entries;

DROP TABLE synchronization_acknowledgement_entries;

ALTER TABLE synchronization_acknowledgement_entries_v3
    RENAME TO synchronization_acknowledgement_entries;
