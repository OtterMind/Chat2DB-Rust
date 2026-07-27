import {
  DatabaseZap,
  LoaderCircle,
  X,
} from 'lucide-react';
import {
  FormEvent,
  KeyboardEvent as ReactKeyboardEvent,
  useEffect,
  useRef,
  useState,
} from 'react';

import type {
  BuildCommunityNamespaceSqlRequest,
  CommunityDatabase,
} from './backend';
import {
  CommunityNamespaceDraft,
  CommunityNamespaceOperationKind,
  buildCommunityNamespaceRequest,
  createCommunityNamespaceDraft,
} from './community-namespace-model';

interface CommunityNamespaceDialogProps {
  databaseType: string;
  database?: CommunityDatabase;
  schemaName: string;
  supportsDatabase: boolean;
  supportsSchema: boolean;
  busy: boolean;
  error: string | null;
  onClose: () => void;
  onBuild: (request: BuildCommunityNamespaceSqlRequest) => Promise<void>;
}

const DATABASE_OPERATIONS: ReadonlyArray<{
  kind: CommunityNamespaceOperationKind;
  label: string;
}> = [
  { kind: 'createDatabase', label: 'Create database' },
  { kind: 'alterDatabase', label: 'Alter database' },
  { kind: 'dropDatabase', label: 'Drop database' },
  { kind: 'useDatabase', label: 'Use database' },
];

const SCHEMA_OPERATIONS: ReadonlyArray<{
  kind: CommunityNamespaceOperationKind;
  label: string;
}> = [
  { kind: 'createSchema', label: 'Create schema' },
  { kind: 'alterSchema', label: 'Alter schema' },
  { kind: 'dropSchema', label: 'Drop schema' },
];

function NamespaceInput({
  label,
  value,
  disabled,
  onChange,
}: {
  label: string;
  value: string;
  disabled: boolean;
  onChange: (value: string) => void;
}) {
  return (
    <label>
      <span>{label}</span>
      <input
        aria-label={label}
        value={value}
        disabled={disabled}
        spellCheck={false}
        onChange={(event) => onChange(event.target.value)}
      />
    </label>
  );
}

function NamespaceFields({
  draft,
  busy,
  onChange,
}: {
  draft: CommunityNamespaceDraft;
  busy: boolean;
  onChange: (patch: Partial<CommunityNamespaceDraft>) => void;
}) {
  if (draft.kind === 'createDatabase') {
    return (
      <>
        <NamespaceInput label="Database name" value={draft.databaseName} disabled={busy} onChange={(databaseName) => onChange({ databaseName })} />
        <div className="form-grid two-columns">
          <NamespaceInput label="Owner" value={draft.owner} disabled={busy} onChange={(owner) => onChange({ owner })} />
          <NamespaceInput label="Charset" value={draft.charset} disabled={busy} onChange={(charset) => onChange({ charset })} />
          <NamespaceInput label="Collation" value={draft.collation} disabled={busy} onChange={(collation) => onChange({ collation })} />
          <NamespaceInput label="Comment" value={draft.comment} disabled={busy} onChange={(comment) => onChange({ comment })} />
        </div>
      </>
    );
  }
  if (draft.kind === 'alterDatabase') {
    return (
      <>
        <div className="form-grid two-columns">
          <NamespaceInput label="Current database name" value={draft.oldDatabaseName} disabled={busy} onChange={(oldDatabaseName) => onChange({ oldDatabaseName })} />
          <NamespaceInput label="New database name" value={draft.newDatabaseName} disabled={busy} onChange={(newDatabaseName) => onChange({ newDatabaseName })} />
          <NamespaceInput label="Owner" value={draft.owner} disabled={busy} onChange={(owner) => onChange({ owner })} />
          <NamespaceInput label="Charset" value={draft.charset} disabled={busy} onChange={(charset) => onChange({ charset })} />
          <NamespaceInput label="Collation" value={draft.collation} disabled={busy} onChange={(collation) => onChange({ collation })} />
          <NamespaceInput label="Comment" value={draft.comment} disabled={busy} onChange={(comment) => onChange({ comment })} />
        </div>
      </>
    );
  }
  if (draft.kind === 'dropDatabase' || draft.kind === 'useDatabase') {
    return <NamespaceInput label="Database name" value={draft.oldDatabaseName} disabled={busy} onChange={(oldDatabaseName) => onChange({ oldDatabaseName })} />;
  }
  if (draft.kind === 'createSchema') {
    return (
      <>
        <div className="form-grid two-columns">
          <NamespaceInput label="Database name" value={draft.schemaDatabaseName} disabled={busy} onChange={(schemaDatabaseName) => onChange({ schemaDatabaseName })} />
          <NamespaceInput label="Schema name" value={draft.schemaName} disabled={busy} onChange={(schemaName) => onChange({ schemaName })} />
          <NamespaceInput label="Owner" value={draft.owner} disabled={busy} onChange={(owner) => onChange({ owner })} />
          <NamespaceInput label="Comment" value={draft.comment} disabled={busy} onChange={(comment) => onChange({ comment })} />
        </div>
      </>
    );
  }
  if (draft.kind === 'alterSchema') {
    return (
      <div className="form-grid two-columns">
        <NamespaceInput label="Current schema name" value={draft.oldSchemaName} disabled={busy} onChange={(oldSchemaName) => onChange({ oldSchemaName })} />
        <NamespaceInput label="New schema name" value={draft.newSchemaName} disabled={busy} onChange={(newSchemaName) => onChange({ newSchemaName })} />
      </div>
    );
  }
  return <NamespaceInput label="Schema name" value={draft.oldSchemaName} disabled={busy} onChange={(oldSchemaName) => onChange({ oldSchemaName })} />;
}

export function CommunityNamespaceDialog({
  databaseType,
  database,
  schemaName,
  supportsDatabase,
  supportsSchema,
  busy,
  error,
  onClose,
  onBuild,
}: CommunityNamespaceDialogProps) {
  const initialKind: CommunityNamespaceOperationKind = supportsSchema
    ? 'createSchema'
    : 'createDatabase';
  const [draft, setDraft] = useState(() => (
    createCommunityNamespaceDraft(database, schemaName, initialKind)
  ));
  const [validation, setValidation] = useState<string | null>(null);
  const dialogRef = useRef<HTMLElement | null>(null);
  const returnFocusRef = useRef<HTMLElement | null>(
    typeof document === 'undefined' ? null : document.activeElement as HTMLElement | null,
  );

  useEffect(() => () => returnFocusRef.current?.focus(), []);

  const patchDraft = (patch: Partial<CommunityNamespaceDraft>) => {
    setDraft((current) => ({ ...current, ...patch }));
    setValidation(null);
  };

  const handleKeyDown = (event: ReactKeyboardEvent<HTMLElement>) => {
    if (event.key === 'Escape') {
      event.preventDefault();
      onClose();
      return;
    }
    if (event.key !== 'Tab') return;
    const focusable = dialogRef.current?.querySelectorAll<HTMLElement>(
      'button:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])',
    );
    if (!focusable?.length) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const result = buildCommunityNamespaceRequest(databaseType, draft);
    if (!result.ok) {
      setValidation(result.error);
      return;
    }
    setValidation(null);
    await onBuild(result.request);
  };

  return (
    <div className="dialog-backdrop" role="presentation" onMouseDown={onClose}>
      <section
        ref={dialogRef}
        className="dialog namespace-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="namespace-dialog-title"
        onKeyDown={handleKeyDown}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="dialog-header">
          <div>
            <span className="section-kicker">{databaseType}</span>
            <h2 id="namespace-dialog-title">Database and schema SQL</h2>
          </div>
          <button className="icon-button quiet" type="button" onClick={onClose} aria-label="Close" title="Close">
            <X size={18} aria-hidden="true" />
          </button>
        </header>
        <form aria-busy={busy} onSubmit={(event) => void submit(event)}>
          <label>
            <span>Operation</span>
            <select
              autoFocus
              aria-label="Namespace operation"
              value={draft.kind}
              disabled={busy}
              onChange={(event) => patchDraft({ kind: event.target.value as CommunityNamespaceOperationKind })}
            >
              {supportsDatabase ? (
                <optgroup label="Database">
                  {DATABASE_OPERATIONS.map((operation) => <option value={operation.kind} key={operation.kind}>{operation.label}</option>)}
                </optgroup>
              ) : null}
              {supportsSchema ? (
                <optgroup label="Schema">
                  {SCHEMA_OPERATIONS.map((operation) => <option value={operation.kind} key={operation.kind}>{operation.label}</option>)}
                </optgroup>
              ) : null}
            </select>
          </label>

          <div className="namespace-fields">
            <NamespaceFields draft={draft} busy={busy} onChange={patchDraft} />
          </div>

          {validation || error ? <p className="form-error" role="alert">{validation ?? error}</p> : null}
          <footer className="dialog-actions">
            <button className="secondary-button" type="button" onClick={onClose}>Cancel</button>
            <button className="primary-button" type="submit" disabled={busy}>
              {busy ? <LoaderCircle className="spinning" size={16} aria-hidden="true" /> : <DatabaseZap size={16} aria-hidden="true" />}
              {busy ? 'Building' : 'Use SQL'}
            </button>
          </footer>
        </form>
      </section>
    </div>
  );
}
