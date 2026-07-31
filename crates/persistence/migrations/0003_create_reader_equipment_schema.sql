-- TransitGuard PostgreSQL storage for project-owned reader equipment.
--
-- This table stores public equipment identity metadata only. It never stores
-- private keys, authentication secrets, or real transit-equipment credentials.

CREATE TABLE reader_equipment (
    id UUID PRIMARY KEY,
    equipment_key_id UUID NOT NULL UNIQUE,
    status TEXT NOT NULL,
    disablement_reason TEXT,
    revocation_reason TEXT,
    aggregate_version BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,

    CONSTRAINT reader_equipment_id_not_nil_check
        CHECK (
            id
            <> '00000000-0000-0000-0000-000000000000'::UUID
        ),

    CONSTRAINT reader_equipment_id_version_check
        CHECK (
            SUBSTRING(id::TEXT FROM 15 FOR 1) = '7'
        ),

    CONSTRAINT reader_equipment_id_variant_check
        CHECK (
            SUBSTRING(id::TEXT FROM 20 FOR 1)
            IN ('8', '9', 'a', 'b')
        ),

    CONSTRAINT reader_equipment_key_id_not_nil_check
        CHECK (
            equipment_key_id
            <> '00000000-0000-0000-0000-000000000000'::UUID
        ),

    CONSTRAINT reader_equipment_key_id_version_check
        CHECK (
            SUBSTRING(
                equipment_key_id::TEXT
                FROM 15 FOR 1
            ) = '7'
        ),

    CONSTRAINT reader_equipment_key_id_variant_check
        CHECK (
            SUBSTRING(
                equipment_key_id::TEXT
                FROM 20 FOR 1
            ) IN ('8', '9', 'a', 'b')
        ),

    CONSTRAINT reader_equipment_status_check
        CHECK (
            status IN (
                'pending_registration',
                'active',
                'offline',
                'disabled',
                'revoked',
                'decommissioned'
            )
        ),

    CONSTRAINT reader_equipment_disablement_reason_check
        CHECK (
            disablement_reason IS NULL
            OR disablement_reason IN (
                'suspected_compromise',
                'lost_equipment',
                'invalid_configuration',
                'administrative_action',
                'test_cleanup'
            )
        ),

    CONSTRAINT reader_equipment_revocation_reason_check
        CHECK (
            revocation_reason IS NULL
            OR revocation_reason IN (
                'suspected_compromise',
                'credential_exposure',
                'administrative_action',
                'security_incident',
                'test_cleanup'
            )
        ),

    CONSTRAINT reader_equipment_reason_state_check
        CHECK (
            (
                status = 'disabled'
                AND disablement_reason IS NOT NULL
                AND revocation_reason IS NULL
            )
            OR (
                status = 'revoked'
                AND disablement_reason IS NULL
                AND revocation_reason IS NOT NULL
            )
            OR (
                status NOT IN ('disabled', 'revoked')
                AND disablement_reason IS NULL
                AND revocation_reason IS NULL
            )
        ),

    CONSTRAINT reader_equipment_version_check
        CHECK (aggregate_version > 0)
);

CREATE INDEX reader_equipment_status_index
    ON reader_equipment (status);
