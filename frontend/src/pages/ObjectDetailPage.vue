<script setup lang="ts">
import { computed } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import ObjectDetail from '../components/ObjectDetail.vue';
const route = useRoute();
const router = useRouter();
const objectId = computed((): string => String(route.params.id ?? ''));
</script>
<template>
  <div class="page-head">
    <div>
      <h1 class="page-title">
        对象详情
      </h1><p class="page-desc">
        核对来源、观察起点与记录缺口。
      </p>
    </div><button
      class="btn"
      @click="router.back()"
    >
      返回列表
    </button>
  </div>
  <ObjectDetail
    :object-id="objectId"
    standalone
  />
</template>
