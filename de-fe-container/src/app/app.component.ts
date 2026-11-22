import { Component } from '@angular/core';
import { CommonModule } from '@angular/common';
import { RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';

interface StudioNavItem {
  label: string;
  route: string;
  wrapperName: string;
}

@Component({
  selector: 'de-root',
  standalone: true,
  imports: [CommonModule, RouterOutlet, RouterLink, RouterLinkActive],
  template: `
    <div class="studio-shell" data-testid="de-studio-shell">
      <aside class="studio-sidebar">
        <div class="brand">
          <span class="brand-title">Data-Engine Studio</span>
          <span class="brand-sub">Visuals &amp; Semantic Topology</span>
        </div>

        <nav class="nav-list" aria-label="Data-Engine Studio Views">
          <a
            *ngFor="let item of navItems"
            class="nav-link"
            [routerLink]="[item.route]"
            routerLinkActive="active"
            [attr.data-testid]="'nav-' + item.route">
            <span class="nav-label">{{ item.label }}</span>
            <span class="nav-wrapper">{{ item.wrapperName }}</span>
          </a>
        </nav>

        <div class="rhs-switcher" data-testid="rhs-canvas-switcher">
          <div class="switcher-title">RHS Shared Canvas Wrappers (7)</div>
          <div class="switcher-pills">
            <a
              *ngFor="let item of navItems"
              class="switcher-pill"
              [routerLink]="[item.route]"
              routerLinkActive="active">
              ./{{ item.wrapperName }}
            </a>
          </div>
        </div>
      </aside>

      <main class="studio-workspace">
        <router-outlet></router-outlet>
      </main>
    </div>
  `,
  styles: [`
    :host { display: block; height: 100vh; overflow: hidden; }
    .studio-shell {
      display: grid;
      grid-template-columns: 260px 1fr;
      height: 100vh;
      overflow: hidden;
      background: #020617;
      color: #f8fafc;
    }
    .studio-sidebar {
      display: flex;
      flex-direction: column;
      gap: 14px;
      padding: 16px 12px;
      background: #0b1120;
      border-right: 1px solid #1e293b;
      overflow-y: auto;
    }
    .brand { display: flex; flex-direction: column; gap: 2px; padding: 4px 8px; }
    .brand-title { font-size: 15px; font-weight: 700; color: #f8fafc; }
    .brand-sub { font-size: 11px; color: #94a3b8; }
    .nav-list { display: flex; flex-direction: column; gap: 4px; }
    .nav-link {
      display: flex;
      flex-direction: column;
      padding: 8px 10px;
      border-radius: 8px;
      color: #cbd5e1;
      text-decoration: none;
      border: 1px solid transparent;
    }
    .nav-link:hover { background: #1e293b; color: #f8fafc; }
    .nav-link.active {
      background: rgba(37, 99, 235, 0.22);
      border-color: #3b82f6;
      color: #ffffff;
    }
    .nav-label { font-size: 13px; font-weight: 600; }
    .nav-wrapper { font-size: 10px; color: #94a3b8; font-family: monospace; }
    .rhs-switcher {
      margin-top: auto;
      padding-top: 12px;
      border-top: 1px solid #1e293b;
      display: flex;
      flex-direction: column;
      gap: 6px;
    }
    .switcher-title { font-size: 11px; color: #94a3b8; font-weight: 600; text-transform: uppercase; }
    .switcher-pills { display: flex; flex-wrap: wrap; gap: 4px; }
    .switcher-pill {
      font-size: 10px;
      font-family: monospace;
      padding: 3px 6px;
      border-radius: 4px;
      background: #1e293b;
      color: #93c5fd;
      text-decoration: none;
      border: 1px solid #334155;
    }
    .switcher-pill.active { background: #2563eb; color: #fff; border-color: #3b82f6; }
    .studio-workspace { height: 100vh; overflow: hidden; display: flex; flex-direction: column; }
  `],
})
export class AppComponent {
  readonly navItems: StudioNavItem[] = [
    { label: 'Data Visual Explorer', route: 'explore', wrapperName: 'DataVisualExplorerWrapper' },
    { label: 'Semantic Topology', route: 'topology', wrapperName: 'SemanticTopologyWrapper' },
    { label: 'Dataset Steward Form', route: 'datasets', wrapperName: 'DataAssetFormWrapper' },
    { label: 'Join Hypothesis Review', route: 'joins', wrapperName: 'JoinHypothesisReviewWrapper' },
    { label: 'Algorithm Sandbox', route: 'algorithms', wrapperName: 'AlgorithmSandboxWrapper' },
    { label: 'Provenance & Lineage', route: 'provenance', wrapperName: 'ProvenanceLineageWrapper' },
    { label: 'Statistical Diagnostics', route: 'diagnostics', wrapperName: 'StatisticalDiagnosticsWrapper' },
  ];
}
