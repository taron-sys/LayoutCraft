// Document schema versioning and migration.
//
// The JSON document is the portable interchange between the browser app, the
// golden fixtures and a future native port. Bump SCHEMA_VERSION whenever the
// persisted shape of the document changes in a way an older reader cannot
// understand, and add a migration keyed by the *old* version.

export const SCHEMA_VERSION = 1;

/**
 * Migration functions from version N to N+1. Version 0 means a document
 * written before schemaVersion existed; the initial versioned model is the
 * same shape, so 0 -> 1 only adds the marker.
 */
const MIGRATIONS = {
  0(doc) {
    return doc;
  },
};

/**
 * Validate/normalise a parsed document and bring it to the current schema.
 * Mutates and returns the same object, matching the importer's existing style.
 */
export function normalizeDocument(raw) {
  if (!raw || typeof raw !== 'object' || !Array.isArray(raw.pages) || !raw.pages.length) {
    throw new Error('Not a LayoutCraft document');
  }
  const sourceVersion = Number.isInteger(raw.schemaVersion) ? raw.schemaVersion : 0;
  if (sourceVersion > SCHEMA_VERSION) {
    throw new Error(
      `This document uses schema version ${sourceVersion}, but this build supports up to ${SCHEMA_VERSION}.`,
    );
  }
  let doc = raw;
  for (let version = sourceVersion; version < SCHEMA_VERSION; version++) {
    const migrate = MIGRATIONS[version];
    if (!migrate) throw new Error(`No migration from document schema ${version}`);
    doc = migrate(doc) || doc;
  }
  doc.schemaVersion = SCHEMA_VERSION;
  for (const page of doc.pages) {
    if (!page || typeof page !== 'object') throw new Error('A page is not an object');
    page.shapes = page.shapes || [];
    page.connectors = page.connectors || [];
    page.groups = page.groups || [];
    page.containers = page.containers || [];
  }
  doc.pageIndex = Math.min(Math.max(0, doc.pageIndex || 0), doc.pages.length - 1);
  return doc;
}

/** Ensure an in-memory document carries the current marker before export/autosave. */
export function ensureDocumentVersion(doc) {
  return normalizeDocument(doc);
}
