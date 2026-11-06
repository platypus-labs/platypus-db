# The test KDC of the `kerberos` profile: MIT Kerberos, nothing else.
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends krb5-kdc krb5-admin-server \
    && rm -rf /var/lib/apt/lists/*
