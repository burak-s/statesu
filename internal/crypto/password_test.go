package crypto

import "testing"

func TestHashPasswordRoundTrip(t *testing.T) {
	hash, err := HashPassword("correct horse battery staple")
	if err != nil {
		t.Fatalf("HashPassword: %v", err)
	}
	if hash == "" {
		t.Fatal("expected non-empty hash")
	}
	if err := CheckPassword(hash, "correct horse battery staple"); err != nil {
		t.Fatalf("CheckPassword on matching password failed: %v", err)
	}
}

func TestHashPasswordProducesDifferentHashes(t *testing.T) {
	h1, err := HashPassword("hunter2")
	if err != nil {
		t.Fatalf("HashPassword: %v", err)
	}
	h2, err := HashPassword("hunter2")
	if err != nil {
		t.Fatalf("HashPassword: %v", err)
	}
	if h1 == h2 {
		t.Fatal("expected distinct salted hashes for the same password")
	}
}

func TestCheckPasswordRejectsWrongPassword(t *testing.T) {
	hash, err := HashPassword("right")
	if err != nil {
		t.Fatalf("HashPassword: %v", err)
	}
	if err := CheckPassword(hash, "wrong"); err == nil {
		t.Fatal("expected error for mismatched password")
	}
}

func TestCheckPasswordRejectsMalformedHash(t *testing.T) {
	if err := CheckPassword("not-a-bcrypt-hash", "whatever"); err == nil {
		t.Fatal("expected error for malformed hash")
	}
}

func TestHashPasswordEmptyString(t *testing.T) {
	hash, err := HashPassword("")
	if err != nil {
		t.Fatalf("HashPassword(\"\"): %v", err)
	}
	if err := CheckPassword(hash, ""); err != nil {
		t.Fatalf("CheckPassword on empty password failed: %v", err)
	}
	if err := CheckPassword(hash, "x"); err == nil {
		t.Fatal("expected mismatch error")
	}
}
