# Sample document

This is a sample document for BitVanes, the zero-trust local PII purification
engine. Drop this into the local dashboard or pipe it through the CLI:

```bash
bitvanes scrub sample.md --rules email,ssn,phone,credit_card,street_address
cat sample.md | bitvanes filter --rules email,ssn --mask-char "*"
```

## Why BitVanes

BitVanes directs, filters, and purifies streams of unstructured and binary data
by stripping sensitive PII before it reaches downstream APIs, databases, or AI
models. Everything runs locally — no cloud uploads, no compliance risk.

## Sample sensitive content

Please reach out to alice@example.com or 555-123-4567 for details.
The account number on file references SSN 123-45-6789 and card
4123 4567 8901 2345. Mail to 123 Main Street, Springfield.

After running through BitVanes, every token above is replaced with a masked
placeholder, a fixed-width mask, or a `[SHA256:...]` content hash — your choice.
