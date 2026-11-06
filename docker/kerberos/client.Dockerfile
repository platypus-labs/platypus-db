# The Linux client of the `kerberos-client` profile: runs the conformance test on the compose
# network, with MIT Kerberos, because a Windows machine outside a domain cannot reach the test
# KDC through SSPI. The source comes in as a volume.
FROM rust:1-bookworm
RUN apt-get update \
    && apt-get install -y --no-install-recommends krb5-user libkrb5-dev libclang-dev clang \
        pkg-config postgresql-client \
    && rm -rf /var/lib/apt/lists/*
