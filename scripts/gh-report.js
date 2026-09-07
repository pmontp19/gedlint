'use strict';

// Renders gedlint JSON reports as GitHub Actions output: inline annotations,
// a step summary grouped by rule code, and step outputs.
//
// Input: a directory holding, per linted file and zero-padded index NNN:
//   NNN.path  raw file path (no trailing newline)
//   NNN.json  gedlint --format json output
//   NNN.exit  gedlint exit code
// Everything else arrives through the environment (see readEnv below).

const fs = require('fs');
const path = require('path');

const SEVERITIES = {
  ERROR: { rank: 0, command: 'error', label: 'error', tag: 'ERROR' },
  WARN: { rank: 1, command: 'warning', label: 'warning', tag: 'WARN ' },
  INFO: { rank: 2, command: 'notice', label: 'notice', tag: 'INFO ' },
};

// Workflow commands are line-based: % and newlines have to be percent-encoded
// or the message is truncated. Property values additionally escape : and ,
// because those separate the properties themselves.
function escapeData(value) {
  return String(value).replace(/%/g, '%25').replace(/\r/g, '%0D').replace(/\n/g, '%0A');
}

function escapeProperty(value) {
  return escapeData(value).replace(/:/g, '%3A').replace(/,/g, '%2C');
}

function escapeTableCell(value) {
  return String(value).replace(/\|/g, '\\|').replace(/[\r\n]+/g, ' ');
}

function readEnv() {
  const limit = Number.parseInt(process.env.GEDLINT_MAX_ANNOTATIONS || '50', 10);
  return {
    reportsDir: process.argv[2],
    annotations: process.env.GEDLINT_ANNOTATIONS !== 'false',
    summary: process.env.GEDLINT_SUMMARY !== 'false',
    format: process.env.GEDLINT_FORMAT === 'json' ? 'json' : 'text',
    maxAnnotations: Number.isNaN(limit) || limit < 0 ? 50 : limit,
    workspace: process.env.GITHUB_WORKSPACE || '',
    summaryPath: process.env.GITHUB_STEP_SUMMARY || '',
    outputPath: process.env.GITHUB_OUTPUT || '',
  };
}

// Annotations only land on the diff when the path is repo-relative.
function relativize(file, workspace) {
  if (!workspace) return file;
  const rel = path.relative(workspace, path.resolve(file));
  return rel && !rel.startsWith('..') && !path.isAbsolute(rel) ? rel : file;
}

function loadReports(dir, workspace) {
  const indexes = fs
    .readdirSync(dir)
    .filter((name) => name.endsWith('.json'))
    .map((name) => name.slice(0, -'.json'.length))
    .sort();

  return indexes.map((index) => {
    const file = fs.readFileSync(path.join(dir, index + '.path'), 'utf8');
    const raw = fs.readFileSync(path.join(dir, index + '.json'), 'utf8');
    let report;
    try {
      report = JSON.parse(raw);
    } catch (err) {
      throw new Error('gedlint produced invalid JSON for ' + file + ': ' + err.message);
    }
    return {
      file,
      display: relativize(file, workspace),
      exit: Number.parseInt(fs.readFileSync(path.join(dir, index + '.exit'), 'utf8').trim(), 10),
      report,
    };
  });
}

function severityOf(diagnostic) {
  return SEVERITIES[diagnostic.severity] || SEVERITIES.INFO;
}

function allDiagnostics(entries) {
  const out = [];
  for (const entry of entries) {
    for (const diagnostic of entry.report.diagnostics || []) {
      out.push({ entry, diagnostic });
    }
  }
  // Worst first, so a truncated annotation list keeps the errors.
  return out.sort(
    (a, b) =>
      severityOf(a.diagnostic).rank - severityOf(b.diagnostic).rank ||
      a.entry.display.localeCompare(b.entry.display) ||
      a.diagnostic.line - b.diagnostic.line
  );
}

function emitAnnotations(items, maxAnnotations) {
  const shown = maxAnnotations === 0 ? items : items.slice(0, maxAnnotations);
  for (const { entry, diagnostic } of shown) {
    const properties = [
      'file=' + escapeProperty(entry.display),
      'line=' + diagnostic.line,
      'title=' + escapeProperty('gedlint ' + diagnostic.code + ' (' + diagnostic.category + ')'),
    ].join(',');
    process.stdout.write(
      '::' + severityOf(diagnostic).command + ' ' + properties + '::' + escapeData(diagnostic.message) + '\n'
    );
  }
  return shown.length;
}

function emitLog(entries, format) {
  for (const entry of entries) {
    const diagnostics = entry.report.diagnostics || [];
    const summary = entry.report.summary || { errors: 0, warnings: 0, infos: 0 };
    const heading =
      entry.display +
      ': ' +
      summary.errors +
      ' error(s), ' +
      summary.warnings +
      ' warning(s), ' +
      summary.infos +
      ' notice(s)';

    process.stdout.write('::group::' + escapeData(heading) + '\n');
    if (format === 'json') {
      process.stdout.write(JSON.stringify(entry.report, null, 2) + '\n');
    } else if (diagnostics.length === 0) {
      process.stdout.write('clean\n');
    } else {
      for (const diagnostic of diagnostics) {
        process.stdout.write(
          severityOf(diagnostic).tag +
            ' [' +
            diagnostic.code +
            ':' +
            diagnostic.category +
            '] ' +
            entry.display +
            ':' +
            diagnostic.line +
            ': ' +
            diagnostic.message +
            '\n'
        );
      }
    }
    process.stdout.write('::endgroup::\n');
  }
}

function groupByCode(items) {
  const groups = new Map();
  for (const { entry, diagnostic } of items) {
    let group = groups.get(diagnostic.code);
    if (!group) {
      group = {
        code: diagnostic.code,
        severity: severityOf(diagnostic),
        category: diagnostic.category,
        count: 0,
        example: '`' + entry.display + ':' + diagnostic.line + '` ' + diagnostic.message,
      };
      groups.set(diagnostic.code, group);
    }
    group.count += 1;
  }
  return [...groups.values()].sort(
    (a, b) => a.severity.rank - b.severity.rank || b.count - a.count || a.code.localeCompare(b.code)
  );
}

function countLabel(totals) {
  return [
    totals.errors + ' ' + (totals.errors === 1 ? 'error' : 'errors'),
    totals.warnings + ' ' + (totals.warnings === 1 ? 'warning' : 'warnings'),
    totals.infos + ' ' + (totals.infos === 1 ? 'notice' : 'notices'),
  ].join(', ');
}

function buildSummary(entries, items, totals, annotated) {
  const lines = ['## gedlint', ''];
  const fileWord = entries.length === 1 ? 'file' : 'files';
  lines.push('**' + countLabel(totals) + '** in ' + entries.length + ' ' + fileWord + '.', '');

  if (items.length === 0) {
    lines.push('No diagnostics.', '');
  } else {
    lines.push('### By rule', '');
    lines.push('| Rule | Severity | Category | Count | Example |');
    lines.push('| --- | --- | --- | --- | --- |');
    for (const group of groupByCode(items)) {
      lines.push(
        '| `' +
          group.code +
          '` | ' +
          group.severity.label +
          ' | ' +
          group.category +
          ' | ' +
          group.count +
          ' | ' +
          escapeTableCell(group.example) +
          ' |'
      );
    }
    lines.push('');
  }

  lines.push('### By file', '');
  lines.push('| File | Errors | Warnings | Notices | GEDCOM | Lines | Individuals | Families |');
  lines.push('| --- | --- | --- | --- | --- | --- | --- | --- |');
  for (const entry of entries) {
    const summary = entry.report.summary || { errors: 0, warnings: 0, infos: 0 };
    lines.push(
      '| `' +
        escapeTableCell(entry.display) +
        '` | ' +
        summary.errors +
        ' | ' +
        summary.warnings +
        ' | ' +
        summary.infos +
        ' | ' +
        (entry.report.version || 'unknown') +
        ' | ' +
        (entry.report.lines || 0) +
        ' | ' +
        (entry.report.individuals || 0) +
        ' | ' +
        (entry.report.families || 0) +
        ' |'
    );
  }
  lines.push('');

  if (annotated < items.length) {
    lines.push(
      '> Annotated the ' +
        annotated +
        ' most severe of ' +
        items.length +
        ' diagnostics. Raise `max-annotations` to change that; the table above is always complete.',
      ''
    );
  }
  return lines.join('\n');
}

function main() {
  const env = readEnv();
  if (!env.reportsDir) throw new Error('usage: gh-report.js <reports-dir>');

  const entries = loadReports(env.reportsDir, env.workspace);
  const items = allDiagnostics(entries);
  const totals = entries.reduce(
    (acc, entry) => {
      const summary = entry.report.summary || {};
      acc.errors += summary.errors || 0;
      acc.warnings += summary.warnings || 0;
      acc.infos += summary.infos || 0;
      return acc;
    },
    { errors: 0, warnings: 0, infos: 0 }
  );

  const annotated = env.annotations ? emitAnnotations(items, env.maxAnnotations) : 0;
  emitLog(entries, env.format);

  if (env.summary && env.summaryPath) {
    fs.appendFileSync(env.summaryPath, buildSummary(entries, items, totals, annotated) + '\n');
  }

  if (env.outputPath) {
    const exitCode = entries.reduce((worst, entry) => Math.max(worst, entry.exit), 0);
    fs.appendFileSync(
      env.outputPath,
      [
        'errors=' + totals.errors,
        'warnings=' + totals.warnings,
        'infos=' + totals.infos,
        'files=' + entries.length,
        'exit-code=' + exitCode,
        '',
      ].join('\n')
    );
  }

  process.stdout.write(countLabel(totals) + ' in ' + entries.length + ' file(s)\n');
}

main();
