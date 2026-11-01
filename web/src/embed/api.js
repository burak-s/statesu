// Builds the public delivery API request URL for the embed. The API origin
// comes from the embed script's own address unless an explicit override is
// configured; blog and post slugs are percent-encoded.

export function apiOriginFromScript(src) {
	return new URL(src).origin;
}

export function apiUrl(origin, blogSlug, postSlug) {
	return `${origin}/public/blogs/${encodeURIComponent(blogSlug)}/posts/${encodeURIComponent(postSlug)}`;
}
