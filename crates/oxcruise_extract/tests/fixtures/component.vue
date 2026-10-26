<template><div /></template>
<script>
import Plain from "./plain";
</script>
<script setup lang="ts">
import type { Props } from "./props";
import Child from "./child.vue";
const lazy = () => import("./lazy");
</script>
<style>
@import "./not-handled.css";
</style>
