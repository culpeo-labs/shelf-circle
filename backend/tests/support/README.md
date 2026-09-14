`rsa_test_key.pem` is a throwaway RSA-2048 keypair generated solely to sign
JWTs in tests (see `mod.rs` and `src/auth.rs`'s test module). It never
protects anything real — do not reuse it outside tests.
