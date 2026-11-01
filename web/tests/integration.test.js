import test from 'node:test';
import assert from 'node:assert/strict';
import { isValidPostPath, originFromSiteAddress } from '../src/lib/integration.js';

// Regression tests for the dashboard's website-integration panel rules: the
// UI keeps its inputs editable and disables copying when these rules fail,
// so the rules themselves must be pinned here.

test('site addresses resolve to origins', () => {
	assert.equal(originFromSiteAddress('burakselcuk.me'), 'https://burakselcuk.me');
	assert.equal(originFromSiteAddress('https://burakselcuk.me'), 'https://burakselcuk.me');
	assert.equal(originFromSiteAddress('http://localhost:5173'), 'http://localhost:5173');
	assert.equal(originFromSiteAddress('  https://Example.com/path  '), 'https://example.com');
	assert.equal(originFromSiteAddress(''), '');
	assert.equal(originFromSiteAddress(null), '');
});

test('unusable site addresses are empty, never partial origins', () => {
	assert.equal(originFromSiteAddress('http://'), '');
	assert.equal(originFromSiteAddress('not a url'), '');
	assert.equal(originFromSiteAddress('javascript:alert(1)'), '', 'non-http schemes are rejected');
});

test('paths are a single segment or the root', () => {
	assert.equal(isValidPostPath('/blog'), true);
	assert.equal(isValidPostPath('/'), true);
	assert.equal(isValidPostPath(''), false, 'empty is not a path');
	assert.equal(isValidPostPath('/blog/posts'), false, 'multi-segment paths are rejected');
	assert.equal(isValidPostPath('/Blog'), false);
	assert.equal(isValidPostPath('blog'), false);
	assert.equal(isValidPostPath('/news/posts'), false);
});
