import { connectionStatus, type ConnectionStatus } from '@silvermine/tauri-plugin-connectivity';

import './styles.css';

function renderLoading(): void {
   document.querySelector<HTMLDivElement>('#app')!.innerHTML = `
      <main class="page">
         <section class="panel">
            <h1>Tauri Plugin Connectivity</h1>
            <p>Loading connection status...</p>
         </section>
      </main>
   `;
}

function renderError(error: unknown): void {
   document.querySelector<HTMLDivElement>('#app')!.innerHTML = `
      <main class="page">
         <section class="panel">
            <h1>Tauri Plugin Connectivity</h1>
            <p class="error">Failed to query connection status.</p>
            <pre id="error-details"></pre>
            <button id="refresh" type="button">Try again</button>
         </section>
      </main>
   `;

   document.querySelector<HTMLPreElement>('#error-details')!.textContent = String(error);

   bindRefresh();
}

function renderStatus(status: ConnectionStatus): void {
   document.querySelector<HTMLDivElement>('#app')!.innerHTML = `
      <main class="page">
         <section class="panel">
            <h1>Tauri Plugin Connectivity</h1>
            <p>Query the plugin and inspect the current network status.</p>
            <dl class="status-grid">
               <div>
                  <dt>Connected</dt>
                  <dd>${status.connected}</dd>
               </div>
               <div>
                  <dt>Connection Type</dt>
                  <dd>${status.connectionType}</dd>
               </div>
               <div>
                  <dt>Metered</dt>
                  <dd>${status.metered}</dd>
               </div>
               <div>
                  <dt>Constrained</dt>
                  <dd>${status.constrained}</dd>
               </div>
            </dl>
            <button id="refresh" type="button">Refresh status</button>
            <h2>Raw response</h2>
            <pre>${JSON.stringify(status, null, 3)}</pre>
         </section>
      </main>
   `;

   bindRefresh();
}

async function loadStatus(): Promise<void> {
   renderLoading();

   try {
      renderStatus(await connectionStatus());
   } catch(error) {
      renderError(error);
   }
}

function bindRefresh(): void {
   document.querySelector<HTMLButtonElement>('#refresh')?.addEventListener('click', () => {
      void loadStatus();
   });
}

void loadStatus();
