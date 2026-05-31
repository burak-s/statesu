package page

import (
	"testing"
	"time"
)

func TestFormatRelativeTime(t *testing.T) {
	now := time.Now()

	cases := []struct {
		name string
		t    time.Time
		want string
	}{
		{"just now", now.Add(-30 * time.Second), "just now"},
		{"1 minute ago", now.Add(-1 * time.Minute), "1 minute ago"},
		{"5 minutes ago", now.Add(-5 * time.Minute), "5 minutes ago"},
		{"1 hour ago", now.Add(-1 * time.Hour), "1 hour ago"},
		{"3 hours ago", now.Add(-3 * time.Hour), "3 hours ago"},
		{"yesterday", now.Add(-30 * time.Hour), "yesterday"},
		{"3 days ago", now.Add(-3 * 24 * time.Hour), "3 days ago"},
		{"old date", now.Add(-10 * 24 * time.Hour), now.Add(-10 * 24 * time.Hour).Format("Jan 2, 2006")},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := formatRelativeTime(tc.t)
			if got != tc.want {
				t.Errorf("formatRelativeTime(%v) = %q, want %q", tc.t, got, tc.want)
			}
		})
	}
}

func TestFormatExpiresAt(t *testing.T) {
	now := time.Now()

	cases := []struct {
		name string
		t    time.Time
		want string
	}{
		{"expired", now.Add(-1 * time.Hour), "expired"},
		{"expires in 1 minute (< 1 min)", now.Add(30 * time.Second), "expires in 1 minute"},
		{"expires in 1 minute (exactly 1 min)", now.Add(1 * time.Minute), "expires in 1 minute"},
		{"expires in 5 minutes", now.Add(5 * time.Minute), "expires in 5 minutes"},
		{"expires in 1 hour", now.Add(1 * time.Hour), "expires in 1 hour"},
		{"expires in 3 hours", now.Add(3 * time.Hour), "expires in 3 hours"},
		{"expires tomorrow", now.Add(30 * time.Hour), "expires tomorrow"},
		{"expires date", now.Add(3 * 24 * time.Hour), "expires " + now.Add(3*24*time.Hour).Format("Jan 2, 2006")},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := formatExpiresAt(tc.t, now)
			if got != tc.want {
				t.Errorf("formatExpiresAt(%v) = %q, want %q", tc.t, got, tc.want)
			}
		})
	}
}
