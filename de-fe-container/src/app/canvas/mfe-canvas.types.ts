import {
  ApplicationRef,
  ComponentRef,
  EnvironmentInjector,
  Type,
  createComponent,
  provideZonelessChangeDetection,
} from '@angular/core';
import { createApplication } from '@angular/platform-browser';
import { provideHttpClient } from '@angular/common/http';

export interface JsonPatchOp {
  op: 'add' | 'replace' | 'remove';
  path: string;
  value?: any;
}

export interface AiFieldAttribution {
  path: string;
  source: 'ai' | 'user';
  confidence?: number;
  rationale?: string;
  epistemicStatus?: 'hypothesized' | 'empirically_tested' | 'verified' | 'rejected';
}

export interface CanvasSelection {
  canvasId: string;
  path: string;
  id?: string;
  label: string;
  context?: any;
}

export interface MfeMountProps<T = any> {
  id?: string;
  title?: string;
  name?: string;
  description?: string;
  content?: T;
  initialState?: T;
  onStateChange?: (state: T, patches: JsonPatchOp[]) => void;
  onSelectionChange?: (selection: CanvasSelection | null) => void;
  onAction?: (actionName: string, payload: any) => void;
}

export interface MfeCanvasController<T = any> {
  applyPatch(patches: JsonPatchOp[] | Partial<T>, attributions?: AiFieldAttribution[]): void;
  getState(): T;
  getSchema(): Record<string, any>;
  unmount(): void;
}

export interface CanvasManagedComponent<T = any> {
  initializeFromProps(props: MfeMountProps<T>): void;
  applyPatch(patches: JsonPatchOp[] | Partial<T>, attributions?: AiFieldAttribution[]): void;
  getState(): T;
  getSchema(): Record<string, any>;
}

/**
 * Pure RFC 6902 JSON Pointer patch applicator supporting both operation arrays
 * (`add`, `replace`, `remove`, including `/-` array append) and partial object merges.
 */
export function applyJsonPatches<T = any>(target: T, patches: JsonPatchOp[] | Partial<T>): T {
  if (target === null || target === undefined || typeof target !== 'object') {
    return !Array.isArray(patches) ? (patches as T) : target;
  }

  const clone: any = JSON.parse(JSON.stringify(target));

  if (!Array.isArray(patches)) {
    if (patches && typeof patches === 'object') {
      return { ...clone, ...patches };
    }
    return clone;
  }

  for (const op of patches) {
    const rawTokens = op.path
      .split('/')
      .slice(1)
      .map((t) => t.replace(/~1/g, '/').replace(/~0/g, '~'));

    if (rawTokens.length === 0) {
      if (op.op === 'add' || op.op === 'replace') {
        return JSON.parse(JSON.stringify(op.value)) as T;
      }
      continue;
    }

    let cursor: any = clone;
    for (let i = 0; i < rawTokens.length - 1; i++) {
      const token = rawTokens[i];
      if (cursor[token] === undefined || cursor[token] === null || typeof cursor[token] !== 'object') {
        cursor[token] = /^\d+$/.test(rawTokens[i + 1]) ? [] : {};
      }
      cursor = cursor[token];
    }

    const last = rawTokens[rawTokens.length - 1];
    if (op.op === 'remove') {
      if (Array.isArray(cursor)) {
        const idx = Number(last);
        if (!Number.isNaN(idx)) cursor.splice(idx, 1);
      } else {
        delete cursor[last];
      }
    } else if (op.op === 'add' || op.op === 'replace') {
      const clonedValue = op.value === undefined ? undefined : JSON.parse(JSON.stringify(op.value));
      if (Array.isArray(cursor)) {
        if (last === '-') {
          cursor.push(clonedValue);
        } else {
          const idx = Number(last);
          if (!Number.isNaN(idx)) {
            if (op.op === 'add') {
              cursor.splice(idx, 0, clonedValue);
            } else {
              cursor[idx] = clonedValue;
            }
          }
        }
      } else {
        cursor[last] = clonedValue;
      }
    }
  }

  return clone as T;
}

/**
 * Reusable Native Federation mount helper implementing `MfeCanvasController`
 * so every wrapper patches live Angular components in-place without unmounting DOM.
 */
export function createCanvasWrapper<T, C extends CanvasManagedComponent<T>>(
  componentType: Type<C>,
) {
  const mounted = new Map<HTMLElement, { appRef: ApplicationRef; compRef: ComponentRef<C> }>();

  function unmount(container: HTMLElement): void {
    const entry = mounted.get(container);
    if (!entry) return;
    entry.compRef.destroy();
    entry.appRef.destroy();
    mounted.delete(container);
  }

  async function mount(container: HTMLElement, props: MfeMountProps<T> = {}): Promise<MfeCanvasController<T>> {
    if (mounted.has(container)) {
      unmount(container);
    }

    const appRef = await createApplication({
      providers: [provideZonelessChangeDetection(), provideHttpClient()],
    });
    const environmentInjector = appRef.injector.get(EnvironmentInjector);
    const compRef = createComponent(componentType, {
      environmentInjector,
      hostElement: container,
    });

    compRef.instance.initializeFromProps(props);
    appRef.attachView(compRef.hostView);
    compRef.changeDetectorRef.detectChanges();

    mounted.set(container, { appRef, compRef });

    return {
      applyPatch(patches: JsonPatchOp[] | Partial<T>, attributions?: AiFieldAttribution[]): void {
        compRef.instance.applyPatch(patches, attributions);
        compRef.changeDetectorRef.detectChanges();
      },
      getState(): T {
        return compRef.instance.getState();
      },
      getSchema(): Record<string, any> {
        return compRef.instance.getSchema();
      },
      unmount(): void {
        unmount(container);
      },
    };
  }

  return { mount, unmount };
}
