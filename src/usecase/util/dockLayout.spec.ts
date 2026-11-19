import { test, expect, describe } from 'bun:test';

import {
  hidePanel,
  showPanel,
  movePanel,
  resetLayout,
  setActivePanel,
  visiblePanels,
  normalizeLayout,
  DEFAULT_LAYOUT
} from '~/usecase/util/dockLayout';

describe('normalizeLayout', () => {
  test('returns default for garbage input', () => {
    const result = normalizeLayout('project', 'garbage');
    expect(result).toEqual(DEFAULT_LAYOUT.project);
  });

  test('returns default for null input', () => {
    const result = normalizeLayout('project', null);
    expect(result).toEqual(DEFAULT_LAYOUT.project);
  });

  test('drops ids not in MODE_PANELS and duplicates', () => {
    const raw = { left: ['projects', 'projects', 'unknownPanel'], right: ['files', 'git', 'notes', 'docker'], hidden: [] };
    const result = normalizeLayout('project', raw);
    expect(result.left).toEqual(['projects']);
    expect(result.right).toEqual(['files', 'git', 'notes', 'docker']);
  });

  test('appends missing panel to its default side', () => {
    const raw = { left: ['projects'], right: ['files', 'git', 'notes'], hidden: [] };
    const result = normalizeLayout('project', raw);
    expect(result.right).toContain('docker');
  });

  test('fixes active to first panel when stored active is not on that side', () => {
    const raw = {
      left: ['projects'],
      right: ['files', 'git', 'notes', 'docker'],
      hidden: [],
      active: { left: 'files', right: 'git' }
    };
    const result = normalizeLayout('project', raw);
    expect(result.active.left).toBe('projects');
    expect(result.active.right).toBe('git');
  });

  test('active is null when side is empty', () => {
    const raw = {
      left: ['projects'],
      right: [],
      hidden: ['files', 'git', 'notes', 'docker'],
      active: { left: 'projects', right: 'files' }
    };
    const result = normalizeLayout('project', raw);
    expect(result.active.right).toBeNull();
  });
});

describe('normalizeLayout canvas mode tiles migration', () => {
  test('migrates legacy tiles id to projects in left array', () => {
    const raw = { left: ['files', 'tiles', 'git', 'docker'], right: [], hidden: [] };
    const result = normalizeLayout('canvas', raw);
    expect(result.left).not.toContain('tiles');
    expect(result.left).toContain('projects');
  });

  test('migrates tiles in hidden and deduplicates if projects already present', () => {
    const raw = { left: ['files', 'projects', 'git', 'docker'], right: [], hidden: ['tiles'] };
    const result = normalizeLayout('canvas', raw);
    expect(result.left.filter((p) => p === 'projects')).toHaveLength(1);
    expect(result.hidden).not.toContain('tiles');
    expect(result.hidden).not.toContain('projects');
  });

  test('migrates tiles in active panel id', () => {
    const raw = { left: ['files', 'tiles', 'git', 'docker'], right: [], hidden: [], active: { left: 'tiles', right: null } };
    const result = normalizeLayout('canvas', raw);
    expect(result.active.left).toBe('projects');
  });
});

describe('movePanel', () => {
  test('moves panel from right to left and makes it active there', () => {
    const layout = DEFAULT_LAYOUT.project;
    const result = movePanel(layout, 'git', 'left');
    expect(result.left).toContain('git');
    expect(result.right).not.toContain('git');
    expect(result.active.left).toBe('git');
  });

  test('moving last panel from a side leaves active null', () => {
    const layout = DEFAULT_LAYOUT.project;
    const moved1 = movePanel(layout, 'files', 'left');
    const moved2 = movePanel(moved1, 'git', 'left');
    const moved3 = movePanel(moved2, 'notes', 'left');
    const result = movePanel(moved3, 'docker', 'left');
    expect(result.right).toHaveLength(0);
    expect(result.active.right).toBeNull();
  });

  test('moving active panel updates active on source side', () => {
    const layout = DEFAULT_LAYOUT.project;
    expect(layout.active.right).toBe('files');
    const result = movePanel(layout, 'files', 'left');
    expect(result.active.right).toBe('git');
  });
});

describe('hidePanel and showPanel', () => {
  test('hidePanel moves panel to hidden and updates active', () => {
    const layout = DEFAULT_LAYOUT.project;
    const result = hidePanel(layout, 'files');
    expect(result.hidden).toContain('files');
    expect(result.right).not.toContain('files');
    expect(result.active.right).toBe('git');
  });

  test('showPanel restores panel to its default side', () => {
    const layout = DEFAULT_LAYOUT.project;
    const hidden = hidePanel(layout, 'files');
    const result = showPanel(hidden, 'files', 'project');
    expect(result.right).toContain('files');
    expect(result.hidden).not.toContain('files');
    expect(result.active.right).toBe('files');
  });

  test('showPanel restores projects to left side', () => {
    const layout = DEFAULT_LAYOUT.project;
    const hidden = hidePanel(layout, 'projects');
    const result = showPanel(hidden, 'projects', 'project');
    expect(result.left).toContain('projects');
    expect(result.active.left).toBe('projects');
  });
});

describe('setActivePanel', () => {
  test('sets active on specified side without affecting the other', () => {
    const layout = DEFAULT_LAYOUT.project;
    const result = setActivePanel(layout, 'right', 'git');
    expect(result.active.right).toBe('git');
    expect(result.active.left).toBe('projects');
  });
});

describe('visiblePanels', () => {
  test('filters by availability', () => {
    const layout = DEFAULT_LAYOUT.project;
    const result = visiblePanels(layout, 'right', (id) => id !== 'docker');
    expect(result).not.toContain('docker');
    expect(result).toContain('files');
  });

  test('returns all panels when all available', () => {
    const layout = DEFAULT_LAYOUT.project;
    const result = visiblePanels(layout, 'right', () => true);
    expect(result).toEqual(layout.right);
  });
});

describe('resetLayout', () => {
  test('returns a copy of the default layout', () => {
    const result = resetLayout('project');
    expect(result).toEqual(DEFAULT_LAYOUT.project);
    result.left.push('git');
    expect(DEFAULT_LAYOUT.project.left).not.toContain('git');
  });
});
