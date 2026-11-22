<script lang="ts">
  // Interrupteur (case à cocher stylée) : même accessibilité qu'une case native.
  let {
    checked,
    label,
    disabled = false,
    title,
    onchange,
  }: { checked: boolean; label: string; disabled?: boolean; title?: string; onchange: (on: boolean) => void } = $props();
</script>

<label class="switch" {title}>
  <input type="checkbox" role="switch" {checked} {disabled} aria-label={label} onchange={(e) => onchange(e.currentTarget.checked)} />
  <span class="track"><span class="knob"></span></span>
</label>

<style>
  .switch {
    position: relative;
    display: inline-grid;
    flex: none;
    cursor: pointer;
  }

  input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    opacity: 0;
    margin: 0;
    cursor: inherit;
  }

  .track {
    display: block;
    width: 36px;
    height: 21px;
    border-radius: 99px;
    background: var(--surface-3);
    border: 1px solid var(--border-strong);
    transition:
      background var(--t) var(--ease),
      border-color var(--t) var(--ease),
      box-shadow var(--t) var(--ease);
  }

  .knob {
    display: block;
    width: 15px;
    height: 15px;
    margin: 2px;
    border-radius: 50%;
    background: var(--muted);
    transition:
      transform var(--t) var(--ease-out),
      background var(--t) var(--ease);
  }

  input:checked + .track {
    background: var(--grad);
    border-color: transparent;
    box-shadow: 0 0 12px -2px rgb(120 100 255 / 60%);
  }

  input:checked + .track .knob {
    transform: translateX(15px);
    background: #fff;
  }

  input:focus-visible + .track {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }

  input:disabled + .track {
    opacity: 0.45;
  }

  .switch:has(input:disabled) {
    cursor: default;
  }
</style>
