import {
  Code2,
  LoaderCircle,
  Plus,
  Trash2,
  X,
} from 'lucide-react';
import {
  FormEvent,
  KeyboardEvent as ReactKeyboardEvent,
  useEffect,
  useRef,
  useState,
} from 'react';

import type { BuildCommunityDmlRequest } from './backend';
import {
  CommunityDmlDraftValue,
  CommunityDmlInsertDraft,
  CommunityDmlMode,
  CommunityDmlPrimaryKey,
  CommunityDmlTableColumn,
  CommunityDmlUpdateDraft,
  addCommunityDmlInsertRow,
  buildCommunityInsertRequest,
  buildCommunityUpdateRequest,
  communityDmlValueKind,
  createCommunityDmlInsertDraft,
  createCommunityDmlUpdateDraft,
} from './community-dml-model';

interface CommunityDmlDialogProps {
  databaseType: string;
  databaseName: string;
  schemaName: string;
  tableName: string;
  columns: CommunityDmlTableColumn[];
  primaryKeys: CommunityDmlPrimaryKey[];
  busy: boolean;
  error: string | null;
  onClose: () => void;
  onBuild: (request: BuildCommunityDmlRequest) => Promise<void>;
}

function inputDescription(columnType: string): {
  type: 'text' | 'date' | 'time' | 'datetime-local';
  inputMode?: 'decimal';
  placeholder?: string;
} {
  const kind = communityDmlValueKind(columnType);
  if (kind === 'decimal') return { type: 'text', inputMode: 'decimal', placeholder: '0' };
  if (kind === 'date') return { type: 'date' };
  if (kind === 'time') return { type: 'time' };
  if (kind === 'localDatetime') return { type: 'datetime-local' };
  if (kind === 'offsetDatetime') return { type: 'text', placeholder: '2026-07-27T12:30:00+08:00' };
  if (kind === 'binary') return { type: 'text', placeholder: 'Base64' };
  return { type: 'text' };
}

function DmlValueInput({
  column,
  value,
  disabled,
  allowNull,
  label,
  onChange,
}: {
  column: CommunityDmlTableColumn;
  value: CommunityDmlDraftValue;
  disabled: boolean;
  allowNull: boolean;
  label: string;
  onChange: (value: CommunityDmlDraftValue) => void;
}) {
  const kind = communityDmlValueKind(column.columnType);
  const input = inputDescription(column.columnType);
  const valueDisabled = disabled || value.isNull;
  return (
    <div className="dml-value-control">
      {kind === 'boolean' ? (
        <select
          aria-label={`${label} value`}
          value={value.input}
          disabled={valueDisabled}
          onChange={(event) => onChange({ ...value, input: event.target.value })}
        >
          <option value="false">false</option>
          <option value="true">true</option>
        </select>
      ) : (
        <input
          aria-label={`${label} value`}
          type={input.type}
          inputMode={input.inputMode}
          placeholder={input.placeholder}
          value={value.input}
          disabled={valueDisabled}
          spellCheck={false}
          onChange={(event) => onChange({ ...value, input: event.target.value })}
        />
      )}
      {allowNull ? (
        <label className="dml-null-toggle">
          <input
            type="checkbox"
            checked={value.isNull}
            disabled={disabled}
            onChange={(event) => onChange({ ...value, isNull: event.target.checked })}
          />
          <span>NULL</span>
        </label>
      ) : <span className="dml-equality-badge">=</span>}
    </div>
  );
}

function DmlField({
  column,
  enabled,
  value,
  busy,
  allowNull,
  purpose,
  onEnabledChange,
  onValueChange,
}: {
  column: CommunityDmlTableColumn;
  enabled: boolean;
  value: CommunityDmlDraftValue;
  busy: boolean;
  allowNull: boolean;
  purpose: 'INSERT' | 'SET' | 'WHERE';
  onEnabledChange: (enabled: boolean) => void;
  onValueChange: (value: CommunityDmlDraftValue) => void;
}) {
  const label = `${purpose} ${column.name}`;
  return (
    <div className={`dml-field ${enabled ? 'enabled' : ''}`}>
      <label className="dml-column-toggle" title={`${enabled ? 'Exclude' : 'Include'} ${column.name}`}>
        <input
          type="checkbox"
          checked={enabled}
          disabled={busy}
          aria-label={`${enabled ? 'Exclude' : 'Include'} ${column.name}`}
          onChange={(event) => onEnabledChange(event.target.checked)}
        />
        <span>
          <strong title={column.name}>{column.name}</strong>
          <small title={column.columnType}>{column.columnType || 'Unknown type'}</small>
        </span>
      </label>
      <DmlValueInput
        column={column}
        value={value}
        disabled={busy || !enabled}
        allowNull={allowNull}
        label={label}
        onChange={onValueChange}
      />
    </div>
  );
}

function InsertFields({
  draft,
  busy,
  onChange,
}: {
  draft: CommunityDmlInsertDraft;
  busy: boolean;
  onChange: (draft: CommunityDmlInsertDraft) => void;
}) {
  const nextRowId = useRef(Math.max(0, ...draft.rows.map((row) => row.id)) + 1);

  const setColumnEnabled = (columnName: string, enabled: boolean) => {
    onChange({
      ...draft,
      columns: draft.columns.map((item) => (
        item.column.name === columnName ? { ...item, enabled } : item
      )),
    });
  };
  const setValue = (rowId: number, columnName: string, value: CommunityDmlDraftValue) => {
    onChange({
      ...draft,
      rows: draft.rows.map((row) => row.id === rowId ? {
        ...row,
        values: { ...row.values, [columnName]: value },
      } : row),
    });
  };

  return (
    <div className="dml-insert-rows">
      {draft.rows.map((row, rowIndex) => (
        <section className="dml-row" key={row.id} aria-label={`INSERT row ${rowIndex + 1}`}>
          <header>
            <strong>Row {rowIndex + 1}</strong>
            <button
              className="icon-button compact-button danger"
              type="button"
              disabled={busy || draft.rows.length === 1}
              onClick={() => onChange({
                ...draft,
                rows: draft.rows.filter((item) => item.id !== row.id),
              })}
              aria-label={`Remove row ${rowIndex + 1}`}
              title={`Remove row ${rowIndex + 1}`}
            >
              <Trash2 size={14} aria-hidden="true" />
            </button>
          </header>
          <div className="dml-fields">
            {draft.columns.map(({ column, enabled }) => (
              <DmlField
                key={column.name}
                column={column}
                enabled={enabled}
                value={row.values[column.name]}
                busy={busy}
                allowNull
                purpose="INSERT"
                onEnabledChange={(nextEnabled) => setColumnEnabled(column.name, nextEnabled)}
                onValueChange={(value) => setValue(row.id, column.name, value)}
              />
            ))}
          </div>
        </section>
      ))}
      <button
        className="secondary-button dml-add-row"
        type="button"
        disabled={busy}
        onClick={() => {
          const nextDraft = addCommunityDmlInsertRow(draft, nextRowId.current);
          if (nextDraft !== draft) nextRowId.current += 1;
          onChange(nextDraft);
        }}
      >
        <Plus size={15} aria-hidden="true" />
        Add row
      </button>
    </div>
  );
}

function UpdateFields({
  draft,
  busy,
  onChange,
}: {
  draft: CommunityDmlUpdateDraft;
  busy: boolean;
  onChange: (draft: CommunityDmlUpdateDraft) => void;
}) {
  const updateField = (
    area: 'assignments' | 'predicates',
    columnName: string,
    patch: Partial<CommunityDmlUpdateDraft['assignments'][number]>,
  ) => {
    onChange({
      ...draft,
      [area]: draft[area].map((field) => (
        field.column.name === columnName ? { ...field, ...patch } : field
      )),
    });
  };

  return (
    <div className="dml-update-fields">
      <section className="dml-row" aria-label="UPDATE values">
        <header><strong>SET values</strong><span>{draft.assignments.filter(({ enabled }) => enabled).length} columns</span></header>
        <div className="dml-fields">
          {draft.assignments.map((field) => (
            <DmlField
              key={field.column.name}
              column={field.column}
              enabled={field.enabled}
              value={field.value}
              busy={busy}
              allowNull
              purpose="SET"
              onEnabledChange={(enabled) => updateField('assignments', field.column.name, { enabled })}
              onValueChange={(value) => updateField('assignments', field.column.name, { value })}
            />
          ))}
        </div>
      </section>
      <section className="dml-row dml-predicates" aria-label="UPDATE equality predicates">
        <header><strong>WHERE equality</strong><span>{draft.predicates.filter(({ enabled }) => enabled).length} predicates</span></header>
        <div className="dml-fields">
          {draft.predicates.map((field) => (
            <DmlField
              key={field.column.name}
              column={field.column}
              enabled={field.enabled}
              value={field.value}
              busy={busy}
              allowNull={false}
              purpose="WHERE"
              onEnabledChange={(enabled) => updateField('predicates', field.column.name, { enabled })}
              onValueChange={(value) => updateField('predicates', field.column.name, { value })}
            />
          ))}
        </div>
      </section>
    </div>
  );
}

export function CommunityDmlDialog({
  databaseType,
  databaseName,
  schemaName,
  tableName,
  columns,
  primaryKeys,
  busy,
  error,
  onClose,
  onBuild,
}: CommunityDmlDialogProps) {
  const [mode, setMode] = useState<CommunityDmlMode>('insert');
  const [insertDraft, setInsertDraft] = useState(() => createCommunityDmlInsertDraft(columns));
  const [updateDraft, setUpdateDraft] = useState(() => createCommunityDmlUpdateDraft(columns, primaryKeys));
  const [validation, setValidation] = useState<string | null>(null);
  const dialogRef = useRef<HTMLElement | null>(null);
  const returnFocusRef = useRef<HTMLElement | null>(
    typeof document === 'undefined' ? null : document.activeElement as HTMLElement | null,
  );

  useEffect(() => () => returnFocusRef.current?.focus(), []);

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
    const target = {
      ...(databaseName ? { databaseName } : {}),
      ...(schemaName ? { schemaName } : {}),
      tableName,
    };
    const result = mode === 'insert'
      ? buildCommunityInsertRequest(databaseType, target, insertDraft)
      : buildCommunityUpdateRequest(databaseType, target, updateDraft);
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
        className="dialog dml-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="dml-dialog-title"
        onKeyDown={handleKeyDown}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="dialog-header">
          <div>
            <span className="section-kicker">{databaseName}.{schemaName}</span>
            <h2 id="dml-dialog-title">{tableName}</h2>
          </div>
          <button className="icon-button quiet" type="button" onClick={onClose} aria-label="Close" title="Close">
            <X size={18} aria-hidden="true" />
          </button>
        </header>
        <form aria-busy={busy} onSubmit={(event) => void submit(event)}>
          <fieldset className="secret-choice">
            <legend>Statement</legend>
            <div className="segmented-control dml-mode-control">
              {(['insert', 'update'] as const).map((item) => (
                <label key={item}>
                  <input
                    type="radio"
                    name="dml-mode"
                    value={item}
                    checked={mode === item}
                    disabled={busy}
                    autoFocus={item === 'insert'}
                    onChange={() => { setMode(item); setValidation(null); }}
                  />
                  <span>{item}</span>
                </label>
              ))}
            </div>
          </fieldset>

          <div className="dml-editor">
            {mode === 'insert' ? (
              <InsertFields draft={insertDraft} busy={busy} onChange={(draft) => { setInsertDraft(draft); setValidation(null); }} />
            ) : (
              <UpdateFields draft={updateDraft} busy={busy} onChange={(draft) => { setUpdateDraft(draft); setValidation(null); }} />
            )}
          </div>

          {validation || error ? <p className="form-error" role="alert">{validation ?? error}</p> : null}
          <footer className="dialog-actions">
            <button className="secondary-button" type="button" onClick={onClose}>Cancel</button>
            <button className="primary-button" type="submit" disabled={busy}>
              {busy ? <LoaderCircle className="spinning" size={16} aria-hidden="true" /> : <Code2 size={16} aria-hidden="true" />}
              {busy ? 'Building' : 'Use SQL'}
            </button>
          </footer>
        </form>
      </section>
    </div>
  );
}
