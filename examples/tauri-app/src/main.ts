import {
   connectionStatus,
   supportedConnectionTypes,
   type ConnectionStatus,
   type ConnectionType,
} from '@silvermine/tauri-plugin-connectivity';

import './styles.css';

interface ConnectivitySnapshot {
   status: ConnectionStatus;
   supportedConnectionTypes: ConnectionType[];
}

function renderLoading(): void {
   document.querySelector<HTMLDivElement>('#app')!.innerHTML = `
      <main class="page">
         <section class="panel">
            <h1>Tauri Plugin Connectivity</h1>
            <p>Loading connectivity details...</p>
         </section>
      </main>
   `;
}

function renderError(error: unknown): void {
   document.querySelector<HTMLDivElement>('#app')!.innerHTML = `
      <main class="page">
         <section class="panel">
            <h1>Tauri Plugin Connectivity</h1>
            <p class="error">Failed to query connectivity details.</p>
            <pre id="error-details"></pre>
            <button id="refresh" type="button">Try again</button>
         </section>
      </main>
   `;

   document.querySelector<HTMLPreElement>('#error-details')!.textContent = String(error);

   bindRefresh();
}

function renderSnapshot(snapshot: ConnectivitySnapshot): void {
   const supportedTypes = snapshot.supportedConnectionTypes
      .map((connectionType) => {
         return `<li>${connectionType}</li>`;
      })
      .join('');

   document.querySelector<HTMLDivElement>('#app')!.innerHTML = `
      <main class="page">
         <section class="panel">
            <h1>Tauri Plugin Connectivity</h1>
            <p>Query the plugin and inspect current and supported network transports.</p>
            <dl class="status-grid">
               <div>
                  <dt>Connected</dt>
                  <dd>${snapshot.status.connected}</dd>
               </div>
               <div>
                  <dt>Connection Type</dt>
                  <dd>${snapshot.status.connectionType}</dd>
               </div>
               <div>
                  <dt>Metered</dt>
                  <dd>${snapshot.status.metered}</dd>
               </div>
               <div>
                  <dt>Constrained</dt>
                  <dd>${snapshot.status.constrained}</dd>
               </div>
            </dl>
            <section class="supported-section" aria-labelledby="supported-heading">
               <h2 id="supported-heading">Supported Connection Types</h2>
               <ul class="supported-list">
                  ${supportedTypes || '<li class="muted">none reported</li>'}
               </ul>
            </section>
            <button id="refresh" type="button">Refresh</button>
            <h2>Raw response</h2>
            <pre>${JSON.stringify(snapshot, null, 3)}</pre>
         </section>
      </main>
   `;

   bindRefresh();
}

async function loadConnectivity(): Promise<void> {
   renderLoading();

   try {
      const [ status, supportedTypes ] = await Promise.all([
         connectionStatus(),
         supportedConnectionTypes(),
      ]);

      renderSnapshot({
         status,
         supportedConnectionTypes: supportedTypes,
      });
   } catch(error) {
      renderError(error);
   }
}

function bindRefresh(): void {
   document.querySelector<HTMLButtonElement>('#refresh')?.addEventListener('click', () => {
      void loadConnectivity();
   });
}

void loadConnectivity();
