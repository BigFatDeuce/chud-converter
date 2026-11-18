<script lang="ts">
  import logo from "$branding/svg/logo-small.svg";
  import Icon, { type IconName } from "./Icon.svelte";
  import { store, type View } from "../store.svelte";

  const items: { view: View; label: string; icon: IconName }[] = [
    { view: "convert", label: "Convertir", icon: "convert" },
    { view: "engines", label: "Moteurs", icon: "engine" },
    { view: "about", label: "À propos", icon: "info" },
  ];
</script>

<nav>
  <div class="brand">
    <img src={logo} alt="" width="40" height="40" />
    <div>
      <strong>Chud Converter</strong>
      <span>local et honnête</span>
    </div>
  </div>

  <ul>
    {#each items as it (it.view)}
      <li>
        <button class:active={store.view === it.view} onclick={() => (store.view = it.view)} aria-current={store.view === it.view ? "page" : undefined}>
          <Icon name={it.icon} size={17} />
          <span>{it.label}</span>
          {#if it.view === "convert" && store.active > 0}
            <em title="Conversions en attente ou en cours">{store.active}</em>
          {/if}
        </button>
      </li>
    {/each}
  </ul>

  <div class="foot">
    <Icon name="shield" size={14} />
    <span>Tout reste sur cet ordinateur</span>
  </div>
</nav>

<style>
  nav {
    display: flex;
    flex-direction: column;
    background: var(--sidebar);
    color: var(--sidebar-text);
    padding: 16px 10px;
    gap: 18px;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 6px;
  }

  .brand div {
    display: grid;
    line-height: 1.2;
  }

  .brand strong {
    color: #fff;
    font-size: 15px;
  }

  .brand span {
    font-size: 11.5px;
    color: #f9c83a;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 2px;
  }

  li button {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 36px;
    padding: 0 10px;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: inherit;
    font-weight: 500;
    text-align: left;
  }

  li button:hover {
    background: rgb(255 255 255 / 6%);
    color: #fff;
  }

  li button.active {
    background: var(--sidebar-active);
    color: #fff;
  }

  li span {
    flex: 1;
  }

  em {
    font-style: normal;
    font-size: 11.5px;
    font-weight: 700;
    min-width: 20px;
    height: 20px;
    padding: 0 6px;
    border-radius: 10px;
    display: grid;
    place-items: center;
    background: #f9c83a;
    color: #011128;
  }

  .foot {
    margin-top: auto;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 8px;
    font-size: 11.5px;
    opacity: 0.8;
  }
</style>
