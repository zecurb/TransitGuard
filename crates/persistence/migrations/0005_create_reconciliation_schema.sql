-- TransitGuard PostgreSQL financial reconciliation storage.
--
-- This schema preserves authoritative reconciliation outcomes, immutable
-- evidence, discrepancy lifecycle state, resolution history, and proposed
-- project-owned financial corrections.
--
-- All data is fictional and project-owned. These tables do not perform real
-- payment capture, refunds, bank transfers, card-network settlement, or
-- external accounting-system posting.


CREATE TABLE reconciliation_records (
    reconciliation_id UUID PRIMARY KEY,

    fare_transaction_id UUID NOT NULL,

    source_batch_id UUID,

    reader_id UUID NOT NULL,

    reader_evidence_fingerprint TEXT NOT NULL,

    backend_evidence_fingerprint TEXT NOT NULL,

    reader_evidence_json JSONB NOT NULL,

    backend_evidence_json JSONB NOT NULL,

    reader_policy_id UUID NOT NULL,

    reader_policy_version BIGINT NOT NULL,

    backend_policy_id UUID NOT NULL,

    backend_policy_version BIGINT NOT NULL,

    outcome TEXT NOT NULL,

    status TEXT NOT NULL,

    observed_minor_units BIGINT,

    observed_currency TEXT,

    expected_minor_units BIGINT,

    expected_currency TEXT,

    monetary_difference_minor_units BIGINT,

    monetary_difference_currency TEXT,

    reconciled_at_unix_milliseconds BIGINT NOT NULL,

    created_at TIMESTAMPTZ NOT NULL
        DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT reconciliation_records_transaction_unique
        UNIQUE (fare_transaction_id),

    CONSTRAINT reconciliation_records_id_not_nil
        CHECK (
            reconciliation_id
            <> '00000000-0000-0000-0000-000000000000'::UUID
        ),

    CONSTRAINT reconciliation_records_id_version
        CHECK (
            SUBSTRING(
                reconciliation_id::TEXT
                FROM 15 FOR 1
            ) = '7'
        ),

    CONSTRAINT reconciliation_records_id_variant
        CHECK (
            SUBSTRING(
                reconciliation_id::TEXT
                FROM 20 FOR 1
            ) IN ('8', '9', 'a', 'b')
        ),

    CONSTRAINT reconciliation_records_transaction_not_nil
        CHECK (
            fare_transaction_id
            <> '00000000-0000-0000-0000-000000000000'::UUID
        ),

    CONSTRAINT reconciliation_records_reader_not_nil
        CHECK (
            reader_id
            <> '00000000-0000-0000-0000-000000000000'::UUID
        ),

    CONSTRAINT reconciliation_records_reader_version
        CHECK (
            SUBSTRING(
                reader_id::TEXT
                FROM 15 FOR 1
            ) = '7'
        ),

    CONSTRAINT reconciliation_records_reader_variant
        CHECK (
            SUBSTRING(
                reader_id::TEXT
                FROM 20 FOR 1
            ) IN ('8', '9', 'a', 'b')
        ),

    CONSTRAINT reconciliation_records_reader_policy_not_nil
        CHECK (
            reader_policy_id
            <> '00000000-0000-0000-0000-000000000000'::UUID
        ),

    CONSTRAINT reconciliation_records_backend_policy_not_nil
        CHECK (
            backend_policy_id
            <> '00000000-0000-0000-0000-000000000000'::UUID
        ),

    CONSTRAINT reconciliation_records_policy_versions
        CHECK (
            reader_policy_version > 0
            AND backend_policy_version > 0
        ),

    CONSTRAINT reconciliation_records_reader_fingerprint
        CHECK (
            reader_evidence_fingerprint
            ~ '^v[1-9][0-9]*:[0-9a-f]{64}$'
        ),

    CONSTRAINT reconciliation_records_backend_fingerprint
        CHECK (
            backend_evidence_fingerprint
            ~ '^v[1-9][0-9]*:[0-9a-f]{64}$'
        ),

    CONSTRAINT reconciliation_records_reader_evidence
        CHECK (
            jsonb_typeof(reader_evidence_json) = 'object'
        ),

    CONSTRAINT reconciliation_records_backend_evidence
        CHECK (
            jsonb_typeof(backend_evidence_json) = 'object'
        ),

    CONSTRAINT reconciliation_records_outcome
        CHECK (
            outcome IN (
                'matched',
                'fare_amount_mismatch',
                'policy_version_mismatch',
                'eligibility_mismatch',
                'product_mismatch',
                'transfer_mismatch',
                'fare_cap_mismatch',
                'duplicate_transaction',
                'missing_backend_context',
                'invalid_evidence',
                'manual_review_required'
            )
        ),

    CONSTRAINT reconciliation_records_status
        CHECK (
            status IN (
                'matched',
                'discrepancy',
                'manual_review'
            )
        ),

    CONSTRAINT reconciliation_records_status_matches_outcome
        CHECK (
            (
                outcome = 'matched'
                AND status = 'matched'
            )
            OR (
                outcome IN (
                    'fare_amount_mismatch',
                    'policy_version_mismatch',
                    'eligibility_mismatch',
                    'product_mismatch',
                    'transfer_mismatch',
                    'fare_cap_mismatch'
                )
                AND status = 'discrepancy'
            )
            OR (
                outcome IN (
                    'duplicate_transaction',
                    'missing_backend_context',
                    'invalid_evidence',
                    'manual_review_required'
                )
                AND status = 'manual_review'
            )
        ),

    CONSTRAINT reconciliation_records_observed_money
        CHECK (
            (
                observed_minor_units IS NULL
                AND observed_currency IS NULL
            )
            OR (
                observed_minor_units IS NOT NULL
                AND observed_currency IS NOT NULL
                AND observed_currency
                    ~ '^[A-Z]{3}$'
            )
        ),

    CONSTRAINT reconciliation_records_expected_money
        CHECK (
            (
                expected_minor_units IS NULL
                AND expected_currency IS NULL
            )
            OR (
                expected_minor_units IS NOT NULL
                AND expected_currency IS NOT NULL
                AND expected_currency
                    ~ '^[A-Z]{3}$'
            )
        ),

    CONSTRAINT reconciliation_records_difference_money
        CHECK (
            (
                monetary_difference_minor_units IS NULL
                AND monetary_difference_currency IS NULL
            )
            OR (
                monetary_difference_minor_units IS NOT NULL
                AND monetary_difference_currency IS NOT NULL
                AND monetary_difference_currency
                    ~ '^[A-Z]{3}$'
            )
        ),

    CONSTRAINT reconciliation_records_difference_requires_amounts
        CHECK (
            monetary_difference_minor_units IS NULL
            OR (
                observed_minor_units IS NOT NULL
                AND expected_minor_units IS NOT NULL
                AND observed_currency = expected_currency
                AND monetary_difference_currency
                    = observed_currency
            )
        ),

    CONSTRAINT reconciliation_records_time
        CHECK (
            reconciled_at_unix_milliseconds >= 0
        ),

    CONSTRAINT reconciliation_records_transaction_fk
        FOREIGN KEY (fare_transaction_id)
        REFERENCES synchronization_ingest_transactions (
            fare_transaction_id
        )
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    CONSTRAINT reconciliation_records_reader_fk
        FOREIGN KEY (reader_id)
        REFERENCES reader_equipment (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    CONSTRAINT reconciliation_records_batch_fk
        FOREIGN KEY (source_batch_id)
        REFERENCES synchronization_ingest_batches (batch_id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT
);

CREATE INDEX reconciliation_records_status_index
    ON reconciliation_records (
        status,
        reconciled_at_unix_milliseconds
    );

CREATE INDEX reconciliation_records_outcome_index
    ON reconciliation_records (
        outcome,
        reconciled_at_unix_milliseconds
    );

CREATE INDEX reconciliation_records_reader_index
    ON reconciliation_records (
        reader_id,
        reconciled_at_unix_milliseconds
    );


CREATE TABLE reconciliation_discrepancy_cases (
    discrepancy_case_id UUID PRIMARY KEY,

    reconciliation_id UUID NOT NULL UNIQUE,

    fare_transaction_id UUID NOT NULL,

    reader_id UUID NOT NULL,

    category TEXT NOT NULL,

    state TEXT NOT NULL,

    created_at_unix_milliseconds BIGINT NOT NULL,

    resolution_actor_id UUID,

    resolution_action TEXT,

    resolution_reason TEXT,

    resolved_at_unix_milliseconds BIGINT,

    created_at TIMESTAMPTZ NOT NULL
        DEFAULT CURRENT_TIMESTAMP,

    updated_at TIMESTAMPTZ NOT NULL
        DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT reconciliation_discrepancy_cases_identity
        CHECK (
            discrepancy_case_id = reconciliation_id
        ),

    CONSTRAINT reconciliation_discrepancy_cases_id_not_nil
        CHECK (
            discrepancy_case_id
            <> '00000000-0000-0000-0000-000000000000'::UUID
        ),

    CONSTRAINT reconciliation_discrepancy_cases_id_version
        CHECK (
            SUBSTRING(
                discrepancy_case_id::TEXT
                FROM 15 FOR 1
            ) = '7'
        ),

    CONSTRAINT reconciliation_discrepancy_cases_id_variant
        CHECK (
            SUBSTRING(
                discrepancy_case_id::TEXT
                FROM 20 FOR 1
            ) IN ('8', '9', 'a', 'b')
        ),

    CONSTRAINT reconciliation_discrepancy_cases_category
        CHECK (
            category IN (
                'fare_amount_mismatch',
                'policy_version_mismatch',
                'eligibility_mismatch',
                'product_mismatch',
                'transfer_mismatch',
                'fare_cap_mismatch',
                'duplicate_transaction',
                'missing_backend_context',
                'invalid_evidence',
                'manual_review_required'
            )
        ),

    CONSTRAINT reconciliation_discrepancy_cases_state
        CHECK (
            state IN (
                'open',
                'manual_review',
                'resolved',
                'dismissed'
            )
        ),

    CONSTRAINT reconciliation_discrepancy_cases_creation_time
        CHECK (
            created_at_unix_milliseconds >= 0
        ),

    CONSTRAINT reconciliation_discrepancy_cases_resolution_actor
        CHECK (
            resolution_actor_id IS NULL
            OR (
                resolution_actor_id
                <> '00000000-0000-0000-0000-000000000000'::UUID
                AND SUBSTRING(
                    resolution_actor_id::TEXT
                    FROM 15 FOR 1
                ) = '7'
                AND SUBSTRING(
                    resolution_actor_id::TEXT
                    FROM 20 FOR 1
                ) IN ('8', '9', 'a', 'b')
            )
        ),

    CONSTRAINT reconciliation_discrepancy_cases_resolution_action
        CHECK (
            resolution_action IS NULL
            OR resolution_action IN (
                'resolve',
                'dismiss'
            )
        ),

    CONSTRAINT reconciliation_discrepancy_cases_resolution_reason
        CHECK (
            resolution_reason IS NULL
            OR resolution_reason IN (
                'reader_evidence_confirmed',
                'backend_evidence_confirmed',
                'policy_exception_approved',
                'duplicate_confirmed',
                'test_data_correction',
                'no_financial_impact'
            )
        ),

    CONSTRAINT reconciliation_discrepancy_cases_resolution_time
        CHECK (
            resolved_at_unix_milliseconds IS NULL
            OR resolved_at_unix_milliseconds
                >= created_at_unix_milliseconds
        ),

    CONSTRAINT reconciliation_discrepancy_cases_resolution_metadata
        CHECK (
            (
                state IN ('open', 'manual_review')
                AND resolution_actor_id IS NULL
                AND resolution_action IS NULL
                AND resolution_reason IS NULL
                AND resolved_at_unix_milliseconds IS NULL
            )
            OR (
                state = 'resolved'
                AND resolution_actor_id IS NOT NULL
                AND resolution_action = 'resolve'
                AND resolution_reason IS NOT NULL
                AND resolved_at_unix_milliseconds IS NOT NULL
            )
            OR (
                state = 'dismissed'
                AND resolution_actor_id IS NOT NULL
                AND resolution_action = 'dismiss'
                AND resolution_reason IS NOT NULL
                AND resolved_at_unix_milliseconds IS NOT NULL
            )
        ),

    CONSTRAINT reconciliation_discrepancy_cases_updated_time
        CHECK (
            updated_at >= created_at
        ),

    CONSTRAINT reconciliation_discrepancy_cases_reconciliation_fk
        FOREIGN KEY (reconciliation_id)
        REFERENCES reconciliation_records (
            reconciliation_id
        )
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    CONSTRAINT reconciliation_discrepancy_cases_transaction_fk
        FOREIGN KEY (fare_transaction_id)
        REFERENCES synchronization_ingest_transactions (
            fare_transaction_id
        )
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    CONSTRAINT reconciliation_discrepancy_cases_reader_fk
        FOREIGN KEY (reader_id)
        REFERENCES reader_equipment (id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT
);

CREATE INDEX reconciliation_discrepancy_cases_state_index
    ON reconciliation_discrepancy_cases (
        state,
        created_at_unix_milliseconds
    );

CREATE INDEX reconciliation_discrepancy_cases_category_index
    ON reconciliation_discrepancy_cases (
        category,
        state
    );


CREATE TABLE reconciliation_discrepancy_history (
    discrepancy_case_id UUID NOT NULL,

    transition_position INTEGER NOT NULL,

    from_state TEXT NOT NULL,

    to_state TEXT NOT NULL,

    resolution_actor_id UUID NOT NULL,

    resolution_action TEXT NOT NULL,

    resolution_reason TEXT NOT NULL,

    occurred_at_unix_milliseconds BIGINT NOT NULL,

    created_at TIMESTAMPTZ NOT NULL
        DEFAULT CURRENT_TIMESTAMP,

    PRIMARY KEY (
        discrepancy_case_id,
        transition_position
    ),

    CONSTRAINT reconciliation_discrepancy_history_position
        CHECK (
            transition_position >= 0
        ),

    CONSTRAINT reconciliation_discrepancy_history_from_state
        CHECK (
            from_state IN (
                'open',
                'manual_review'
            )
        ),

    CONSTRAINT reconciliation_discrepancy_history_to_state
        CHECK (
            to_state IN (
                'resolved',
                'dismissed'
            )
        ),

    CONSTRAINT reconciliation_discrepancy_history_action
        CHECK (
            resolution_action IN (
                'resolve',
                'dismiss'
            )
        ),

    CONSTRAINT reconciliation_discrepancy_history_action_matches_state
        CHECK (
            (
                resolution_action = 'resolve'
                AND to_state = 'resolved'
            )
            OR (
                resolution_action = 'dismiss'
                AND to_state = 'dismissed'
            )
        ),

    CONSTRAINT reconciliation_discrepancy_history_reason
        CHECK (
            resolution_reason IN (
                'reader_evidence_confirmed',
                'backend_evidence_confirmed',
                'policy_exception_approved',
                'duplicate_confirmed',
                'test_data_correction',
                'no_financial_impact'
            )
        ),

    CONSTRAINT reconciliation_discrepancy_history_actor_not_nil
        CHECK (
            resolution_actor_id
            <> '00000000-0000-0000-0000-000000000000'::UUID
        ),

    CONSTRAINT reconciliation_discrepancy_history_actor_version
        CHECK (
            SUBSTRING(
                resolution_actor_id::TEXT
                FROM 15 FOR 1
            ) = '7'
        ),

    CONSTRAINT reconciliation_discrepancy_history_actor_variant
        CHECK (
            SUBSTRING(
                resolution_actor_id::TEXT
                FROM 20 FOR 1
            ) IN ('8', '9', 'a', 'b')
        ),

    CONSTRAINT reconciliation_discrepancy_history_time
        CHECK (
            occurred_at_unix_milliseconds >= 0
        ),

    CONSTRAINT reconciliation_discrepancy_history_case_fk
        FOREIGN KEY (discrepancy_case_id)
        REFERENCES reconciliation_discrepancy_cases (
            discrepancy_case_id
        )
        ON UPDATE RESTRICT
        ON DELETE RESTRICT
);

CREATE INDEX reconciliation_discrepancy_history_time_index
    ON reconciliation_discrepancy_history (
        discrepancy_case_id,
        occurred_at_unix_milliseconds
    );


CREATE TABLE reconciliation_proposed_adjustments (
    proposed_adjustment_id UUID PRIMARY KEY,

    reconciliation_id UUID NOT NULL UNIQUE,

    fare_transaction_id UUID NOT NULL,

    correction_minor_units BIGINT NOT NULL,

    currency TEXT NOT NULL,

    direction TEXT NOT NULL,

    created_at_unix_milliseconds BIGINT NOT NULL,

    created_at TIMESTAMPTZ NOT NULL
        DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT reconciliation_proposed_adjustments_identity
        CHECK (
            proposed_adjustment_id = reconciliation_id
        ),

    CONSTRAINT reconciliation_proposed_adjustments_id_not_nil
        CHECK (
            proposed_adjustment_id
            <> '00000000-0000-0000-0000-000000000000'::UUID
        ),

    CONSTRAINT reconciliation_proposed_adjustments_id_version
        CHECK (
            SUBSTRING(
                proposed_adjustment_id::TEXT
                FROM 15 FOR 1
            ) = '7'
        ),

    CONSTRAINT reconciliation_proposed_adjustments_id_variant
        CHECK (
            SUBSTRING(
                proposed_adjustment_id::TEXT
                FROM 20 FOR 1
            ) IN ('8', '9', 'a', 'b')
        ),

    CONSTRAINT reconciliation_proposed_adjustments_nonzero
        CHECK (
            correction_minor_units <> 0
        ),

    CONSTRAINT reconciliation_proposed_adjustments_currency
        CHECK (
            currency ~ '^[A-Z]{3}$'
        ),

    CONSTRAINT reconciliation_proposed_adjustments_direction
        CHECK (
            direction IN (
                'increase_recorded_fare',
                'decrease_recorded_fare'
            )
        ),

    CONSTRAINT reconciliation_proposed_adjustments_direction_matches_amount
        CHECK (
            (
                correction_minor_units > 0
                AND direction = 'increase_recorded_fare'
            )
            OR (
                correction_minor_units < 0
                AND direction = 'decrease_recorded_fare'
            )
        ),

    CONSTRAINT reconciliation_proposed_adjustments_time
        CHECK (
            created_at_unix_milliseconds >= 0
        ),

    CONSTRAINT reconciliation_proposed_adjustments_reconciliation_fk
        FOREIGN KEY (reconciliation_id)
        REFERENCES reconciliation_records (
            reconciliation_id
        )
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    CONSTRAINT reconciliation_proposed_adjustments_transaction_fk
        FOREIGN KEY (fare_transaction_id)
        REFERENCES synchronization_ingest_transactions (
            fare_transaction_id
        )
        ON UPDATE RESTRICT
        ON DELETE RESTRICT
);

CREATE INDEX reconciliation_proposed_adjustments_transaction_index
    ON reconciliation_proposed_adjustments (
        fare_transaction_id
    );
