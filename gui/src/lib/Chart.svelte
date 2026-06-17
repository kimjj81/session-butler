<script lang="ts">
  // Chart.js 래퍼 — canvas lifecycle 관리. 다크 테마 기본값.
  import Chart from "chart.js/auto";
  import { onDestroy } from "svelte";

  Chart.defaults.color = "#9aa1a8";
  Chart.defaults.borderColor = "#22282e";
  Chart.defaults.font.family =
    "Inter, -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif";

  let {
    type,
    data,
    options = {},
    height = 220,
  }: {
    type: "bar" | "line" | "doughnut";
    data: any;
    options?: any;
    height?: number;
  } = $props();

  let canvas: HTMLCanvasElement | undefined = $state();
  let chart: Chart | null = null;

  // data/options 가 바뀌면 재렌더. $effect 는 DOM 바인딩(canvas) 이후에 실행.
  $effect(() => {
    void data;
    void options;
    if (!canvas) return;
    chart?.destroy();
    chart = new Chart(canvas, {
      type,
      data,
      options: { responsive: true, maintainAspectRatio: false, ...options },
    });
  });

  onDestroy(() => chart?.destroy());
</script>

<div class="wrap" style="height:{height}px">
  <canvas bind:this={canvas}></canvas>
</div>

<style>
  .wrap {
    position: relative;
    width: 100%;
  }
</style>
